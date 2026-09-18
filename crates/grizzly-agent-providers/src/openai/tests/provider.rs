use futures_util::StreamExt;
use grizzly_agent_core::{CompletionRequest, Message};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

#[test]
fn a_trailing_slash_on_the_base_url_does_not_double_up() {
    let with_slash = OpenAiCompatibleProvider::builder("http://localhost:11434/v1/", "m")
        .build()
        .expect("builds");
    let without_slash = OpenAiCompatibleProvider::builder("http://localhost:11434/v1", "m")
        .build()
        .expect("builds");
    assert_eq!(
        format!("{with_slash:?}"),
        format!("{without_slash:?}"),
        "a trailing slash on the base url must not change the completions url"
    );
}

#[test]
fn an_unparseable_base_url_is_a_configuration_failure() {
    let error = OpenAiCompatibleProvider::builder("not a url", "m")
        .build()
        .expect_err("an unparseable base url must not build");
    assert!(matches!(
        error,
        grizzly_agent_core::ProviderFailure::Configuration(_)
    ));
}

#[tokio::test]
async fn a_successful_call_streams_events_from_the_mock_endpoint() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        "data: [DONE]\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider = OpenAiCompatibleProvider::builder(server.uri(), "test-model")
        .api_key("test-key")
        .build()
        .expect("builds against the mock server");

    let mut stream = provider
        .complete(CompletionRequest::new(vec![Message::user("hi")]))
        .await
        .expect("the mock server answers with a stream");

    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("every item is a successful event in this fixture"));
    }
    assert_eq!(
        events,
        vec![
            grizzly_agent_core::CompletionEvent::TextDelta("hi".to_owned()),
            grizzly_agent_core::CompletionEvent::Finished {
                stop_reason: grizzly_agent_core::StopReason::EndOfTurn,
                raw_stop_reason: "stop".to_owned(),
                model: String::new(),
            },
        ]
    );
}

#[tokio::test]
async fn a_non_success_status_fails_before_the_stream_opens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
        .mount(&server)
        .await;

    let provider = OpenAiCompatibleProvider::builder(server.uri(), "test-model")
        .build()
        .expect("builds against the mock server");

    let outcome = provider
        .complete(CompletionRequest::new(vec![Message::user("hi")]))
        .await;
    let Err(failure) = outcome else {
        panic!("a 500 must fail before any stream is returned");
    };
    let grizzly_agent_core::ProviderFailure::Status { status, .. } = failure else {
        panic!("expected a Status failure");
    };
    assert_eq!(status, 500);
}
