use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use grizzly_agent_core::{
    Completion, CompletionRequest, CompletionStream, Content, Message, Model, Provider,
    ProviderFailure, ScriptedProvider, ScriptedResponse, StopReason, Usage,
};

use crate::case::CaseMeta;
use crate::verdict::{Verdict, VerdictCategory};

use super::*;

/// A case scoring a number the model was asked for.
struct NumberCase {
    meta: CaseMeta,
    expected: i64,
    timeout: CaseTimeout,
}

impl NumberCase {
    fn new(name: &str, expected: i64) -> Self {
        Self {
            meta: CaseMeta::new(name),
            expected,
            timeout: CaseTimeout::Default,
        }
    }

    fn repeats(mut self, count: u32) -> Self {
        self.meta.repeats = Some(count);
        self
    }

    fn with_timeout(mut self, timeout: CaseTimeout) -> Self {
        self.timeout = timeout;
        self
    }
}

impl ResponseEvalCase for NumberCase {
    type Answer = i64;

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_request(&self) -> CompletionRequest {
        CompletionRequest::new(vec![Message::user("what number?")])
    }

    fn parse(&self, completion: &Completion) -> Result<i64, String> {
        let text = completion
            .content
            .iter()
            .find_map(|block| match block {
                Content::Text(text) => Some(text.as_str()),
                Content::Reasoning { .. } | Content::ToolUse(_) | Content::ToolResult(_) => None,
            })
            .ok_or_else(|| "no text block in the reply".to_owned())?;
        text.trim()
            .parse::<i64>()
            .map_err(|err| format!("{text:?} is not a number: {err}"))
    }

    fn score(&self, answer: &i64) -> Verdict {
        if *answer == self.expected {
            Verdict::pass(format!("answered {answer}"))
        } else {
            Verdict::wrong(format!("answered {answer}, expected {}", self.expected))
        }
    }

    fn timeout(&self) -> CaseTimeout {
        self.timeout
    }
}

fn text_completion(text: &str) -> Completion {
    Completion {
        content: vec![Content::Text(text.to_owned())],
        usage: Usage {
            input_tokens: Some(3),
            output_tokens: Some(2),
        },
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "scripted-model".to_owned(),
    }
}

fn model_from(provider: impl Provider + 'static) -> Model {
    Model::builder(Arc::new(provider), "scripted-model")
        .retry_policy(grizzly_agent_core::RetryPolicy::none())
        .build()
}

#[tokio::test]
async fn a_correct_answer_passes_with_no_detail() {
    let provider = ScriptedProvider::new([ScriptedResponse::Completion(text_completion("42"))]);
    let runner = ResponseEvalRunner::builder(model_from(provider), 1).build();
    let case = NumberCase::new("right", 42).repeats(1);

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let report = reports.first().expect("one case must produce one report");

    assert_eq!(
        report.aggregate.passes, 1,
        "a matching answer must count as a pass"
    );
    let repeat = report.repeats.first().expect("one repeat must have run");
    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Pass,
        "a matching answer must verdict Pass"
    );
    assert!(
        repeat.detail.is_null(),
        "a passing repeat must carry no report detail"
    );
    assert!(
        repeat.verdict.usage.is_some(),
        "usage must be recorded even on a passing repeat"
    );
}

#[tokio::test]
async fn a_wrong_answer_keeps_the_raw_reply_in_the_detail() {
    let provider = ScriptedProvider::new([ScriptedResponse::Completion(text_completion("7"))]);
    let runner = ResponseEvalRunner::builder(model_from(provider), 1).build();
    let case = NumberCase::new("wrong", 42).repeats(1);

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Wrong,
        "a mismatched answer must verdict Wrong"
    );
    assert_eq!(
        repeat
            .detail
            .get("raw_reply")
            .and_then(serde_json::Value::as_str),
        Some("7"),
        "a wrong answer's raw reply must be kept in the detail"
    );
}

#[tokio::test]
async fn an_unreadable_reply_is_unparseable() {
    let provider = ScriptedProvider::new([ScriptedResponse::Completion(text_completion("banana"))]);
    let runner = ResponseEvalRunner::builder(model_from(provider), 1).build();
    let case = NumberCase::new("garbled", 42).repeats(1);

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Unparseable,
        "text the parser rejects must verdict Unparseable"
    );
    assert_eq!(
        repeat
            .detail
            .get("raw_reply")
            .and_then(serde_json::Value::as_str),
        Some("banana"),
        "an unparseable reply's raw text must be kept in the detail"
    );
}

#[tokio::test]
async fn a_call_that_fails_before_the_stream_opens_is_unavailable() {
    let provider = ScriptedProvider::new([ScriptedResponse::PreStreamFailure(
        ProviderFailure::Transport {
            provider: "test".to_owned(),
            source: Box::new(std::io::Error::other("connection refused")),
        },
    )]);
    let runner = ResponseEvalRunner::builder(model_from(provider), 1).build();
    let case = NumberCase::new("down", 42).repeats(1);

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Unavailable,
        "a call that never reaches the provider must verdict Unavailable"
    );
    assert!(
        repeat.detail.is_null(),
        "there is no raw reply to keep when the call never returned one"
    );
}

/// Wraps a [`Provider`], adding an artificial delay before delegating and
/// tracking how many calls are in flight at once.
struct TrackingProvider {
    inner: ScriptedProvider,
    delay: Duration,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl Provider for TrackingProvider {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure> {
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(current, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        let result = self.inner.complete(request).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

#[tokio::test]
async fn a_slow_reply_beyond_its_case_timeout_is_unavailable() {
    let provider = TrackingProvider {
        inner: ScriptedProvider::new([ScriptedResponse::Completion(text_completion("42"))]),
        delay: Duration::from_millis(200),
        in_flight: Arc::new(AtomicUsize::new(0)),
        max_in_flight: Arc::new(AtomicUsize::new(0)),
    };
    let runner = ResponseEvalRunner::builder(model_from(provider), 1).build();
    let case = NumberCase::new("slow", 42)
        .repeats(1)
        .with_timeout(CaseTimeout::Custom(Duration::from_millis(20)));

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Unavailable,
        "a reply slower than the case's timeout must verdict Unavailable"
    );
    assert!(
        repeat.verdict.reason.contains("within"),
        "the timeout reason must explain what elapsed: {}",
        repeat.verdict.reason
    );
}

#[tokio::test]
async fn repeats_run_concurrently_but_never_past_the_limit() {
    let responses = (0..6).map(|_| ScriptedResponse::Completion(text_completion("1")));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let provider = TrackingProvider {
        inner: ScriptedProvider::new(responses),
        delay: Duration::from_millis(30),
        in_flight: Arc::new(AtomicUsize::new(0)),
        max_in_flight: Arc::clone(&max_in_flight),
    };
    let runner = ResponseEvalRunner::builder(model_from(provider), 2).build();
    let case = NumberCase::new("many", 1).repeats(6);

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let report = reports.first().expect("one case must produce one report");

    assert_eq!(report.repeats.len(), 6, "all six repeats must have run");
    assert!(
        max_in_flight.load(Ordering::SeqCst) <= 2,
        "concurrency must never exceed the configured limit of 2, saw {}",
        max_in_flight.load(Ordering::SeqCst)
    );
    assert!(
        max_in_flight.load(Ordering::SeqCst) >= 2,
        "with 6 repeats and a limit of 2, at least two calls must overlap"
    );
}

#[tokio::test]
async fn a_case_uses_the_suite_default_repeats_when_it_sets_none() {
    let responses = (0..5).map(|_| ScriptedResponse::Completion(text_completion("1")));
    let runner = ResponseEvalRunner::builder(model_from(ScriptedProvider::new(responses)), 1)
        .default_repeats(5)
        .build();
    let case = NumberCase::new("default-repeats", 1);

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let report = reports.first().expect("one case must produce one report");
    assert_eq!(
        report.repeats.len(),
        5,
        "a case with no repeats override must run the runner's default_repeats"
    );
}
