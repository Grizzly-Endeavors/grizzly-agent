use std::sync::Mutex;

use futures_util::StreamExt;

use super::*;
use crate::message::{Content, Message};
use crate::test_support::{ScriptedProvider, ScriptedResponse};

fn sample_transport_failure() -> ProviderFailure {
    ProviderFailure::Transport {
        provider: "test".to_owned(),
        source: Box::new(std::io::Error::other("boom")),
    }
}

fn sample_completion(text: &str) -> Completion {
    Completion {
        content: vec![Content::Text(text.to_owned())],
        usage: crate::completion::Usage::default(),
        stop_reason: crate::completion::StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "test-model".to_owned(),
    }
}

fn instant_sleep() -> SleepFn {
    Arc::new(|_duration| Box::pin(async {}))
}

fn identity_jitter() -> JitterFn {
    Arc::new(|duration| duration)
}

fn recording_sleep(log: Arc<Mutex<Vec<Duration>>>) -> SleepFn {
    Arc::new(move |duration| {
        let log = Arc::clone(&log);
        Box::pin(async move {
            log.lock()
                .expect("sleep log mutex must not be poisoned")
                .push(duration);
        })
    })
}

fn fast_model(provider: ScriptedProvider, retry_policy: RetryPolicy) -> Model {
    Model::builder(Arc::new(provider), "test-model")
        .retry_policy(retry_policy)
        .with_sleep(instant_sleep())
        .with_jitter(identity_jitter())
        .build()
}

fn request() -> CompletionRequest {
    CompletionRequest::new(vec![Message::user("hi")])
}

#[tokio::test]
async fn complete_retries_the_whole_request_on_a_mid_stream_failure() {
    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::Events(vec![
            Ok(CompletionEvent::TextDelta("partial".to_owned())),
            Err(sample_transport_failure()),
        ]),
        ScriptedResponse::Completion(sample_completion("final")),
    ]);
    let model = fast_model(
        provider.clone(),
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
    );

    let completion = model
        .complete(request())
        .await
        .expect("must retry to success");

    assert_eq!(completion.content, vec![Content::Text("final".to_owned())]);
    assert_eq!(
        provider.calls_served(),
        2,
        "a mid-stream failure must retry the whole request"
    );
}

#[tokio::test]
async fn complete_does_not_retry_a_non_retryable_failure() {
    let provider = ScriptedProvider::new(vec![ScriptedResponse::PreStreamFailure(
        ProviderFailure::Configuration("bad key".to_owned()),
    )]);
    let model = fast_model(provider.clone(), RetryPolicy::default());

    let error = model.complete(request()).await.expect_err("must fail");

    assert!(matches!(error, ProviderFailure::Configuration(_)));
    assert_eq!(
        provider.calls_served(),
        1,
        "a non-retryable failure must not be retried"
    );
}

#[tokio::test]
async fn stream_retries_a_pre_first_event_failure() {
    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::PreStreamFailure(sample_transport_failure()),
        ScriptedResponse::Completion(sample_completion("ok")),
    ]);
    let model = fast_model(
        provider.clone(),
        RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(1)),
    );

    let mut stream = model
        .stream(request())
        .await
        .expect("must retry to an opened stream");
    let first = stream
        .next()
        .await
        .expect("a first event")
        .expect("the first event must be ok");

    assert_eq!(first, CompletionEvent::TextDelta("ok".to_owned()));
    assert_eq!(
        provider.calls_served(),
        2,
        "must have retried the failed open"
    );
}

#[tokio::test]
async fn stream_does_not_retry_a_failure_after_the_first_event() {
    let provider = ScriptedProvider::new(vec![ScriptedResponse::Events(vec![
        Ok(CompletionEvent::TextDelta("first".to_owned())),
        Err(sample_transport_failure()),
    ])]);
    let model = fast_model(provider.clone(), RetryPolicy::default());

    let mut stream = model
        .stream(request())
        .await
        .expect("must open on the first attempt");
    let first = stream
        .next()
        .await
        .expect("a first event")
        .expect("the first event must be ok");
    assert_eq!(first, CompletionEvent::TextDelta("first".to_owned()));

    let second = stream.next().await.expect("a second item");
    assert!(
        second.is_err(),
        "a failure after the first event must be yielded, not retried"
    );
    assert_eq!(
        provider.calls_served(),
        1,
        "must not have opened a second stream"
    );
}

#[tokio::test]
async fn invalid_request_is_rejected_before_the_provider_is_called() {
    let provider = ScriptedProvider::new(Vec::new());
    let model = fast_model(provider.clone(), RetryPolicy::default());
    let invalid = CompletionRequest::new(vec![Message::user("hi"), Message::system("late")]);

    let error = model
        .complete(invalid)
        .await
        .expect_err("an invalid request must be rejected");

    assert!(matches!(error, ProviderFailure::InvalidRequest(_)));
    assert_eq!(
        provider.calls_served(),
        0,
        "the provider must never be called for an invalid request"
    );
}

#[tokio::test]
async fn complete_sleeps_the_decide_computed_jittered_delay_per_retry() {
    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::PreStreamFailure(sample_transport_failure()),
        ScriptedResponse::PreStreamFailure(sample_transport_failure()),
        ScriptedResponse::Completion(sample_completion("ok")),
    ]);
    let sleep_log = Arc::new(Mutex::new(Vec::new()));
    let model = Model::builder(Arc::new(provider), "test-model")
        .retry_policy(RetryPolicy::new(
            5,
            Duration::from_millis(500),
            Duration::from_secs(10),
        ))
        .with_sleep(recording_sleep(Arc::clone(&sleep_log)))
        .with_jitter(identity_jitter())
        .build();

    model
        .complete(request())
        .await
        .expect("must retry to success");

    assert_eq!(
        *sleep_log
            .lock()
            .expect("sleep log mutex must not be poisoned"),
        vec![Duration::from_millis(500), Duration::from_millis(1000)],
        "each retry must sleep for decide()'s computed delay"
    );
}

#[tokio::test(start_paused = true)]
async fn complete_times_out_across_retries() {
    let provider = ScriptedProvider::new(
        std::iter::repeat_with(|| ScriptedResponse::PreStreamFailure(sample_transport_failure()))
            .take(10),
    );
    let model = Model::builder(Arc::new(provider), "test-model")
        .retry_policy(RetryPolicy::new(
            10,
            Duration::from_secs(1),
            Duration::from_secs(1),
        ))
        .timeout(Duration::from_secs(3))
        .build();

    let error = model
        .complete(request())
        .await
        .expect_err("the whole call must time out");

    assert!(matches!(error, ProviderFailure::Transport { .. }));
    let source = std::error::Error::source(&error).expect("a timeout must carry a cause");
    assert!(
        source.to_string().contains("timed out"),
        "must name the timeout, got {source}"
    );
}

#[tokio::test(start_paused = true)]
async fn bound_by_deadline_yields_one_timeout_item_then_ends() {
    let never: CompletionStream = futures_util::stream::pending().boxed();
    let deadline = Deadline {
        at: Instant::now() + Duration::from_secs(1),
    };

    let mut bounded = bound_by_deadline(never, deadline, "test-model".to_owned());

    let first = bounded.next().await.expect("a timeout item");
    let error = first.expect_err("the timeout item must be an Err");
    let source = std::error::Error::source(&error).expect("a timeout must carry a cause");
    assert!(source.to_string().contains("timed out"), "got {source}");

    assert!(
        bounded.next().await.is_none(),
        "the stream must end after the timeout item"
    );
}

#[tokio::test]
async fn advertised_tools_reach_the_provider_unchanged() {
    let provider =
        ScriptedProvider::new(vec![ScriptedResponse::Completion(sample_completion("ok"))]);
    let model = fast_model(provider.clone(), RetryPolicy::default());
    let mut with_tools = request();
    with_tools.tools.push(crate::tools::ToolSpec {
        name: std::borrow::Cow::Borrowed("read_file"),
        description: std::borrow::Cow::Borrowed("reads a file"),
        parameters: serde_json::json!({"type": "object"}),
    });

    model.complete(with_tools).await.expect("must succeed");

    let recorded = provider.requests();
    let sent = recorded.first().expect("the request the provider received");
    assert_eq!(
        sent.tools.len(),
        1,
        "the advertised tool must reach the provider"
    );
    assert_eq!(
        sent.tools.first().expect("the recorded tool spec").name,
        "read_file"
    );
}
