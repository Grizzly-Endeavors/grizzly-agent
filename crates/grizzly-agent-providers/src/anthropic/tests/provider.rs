use futures_util::StreamExt;
use grizzly_agent_core::{CompletionRequest, Message};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

#[test]
fn an_unparseable_base_url_is_a_configuration_failure() {
    let error = AnthropicProvider::builder("key")
        .base_url("not a url")
        .build()
        .expect_err("an unparseable base url must not build");
    assert!(matches!(
        error,
        grizzly_agent_core::ProviderFailure::Configuration(_)
    ));
}

#[tokio::test]
async fn a_successful_call_streams_events_and_sends_the_documented_headers() {
    let server = MockServer::start().await;
    let body = concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"text_delta\",\"text\":\"hi\"}}\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n",
        "data: {\"type\":\"message_stop\"}\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;

    let provider = AnthropicProvider::builder("test-key")
        .base_url(server.uri())
        .build()
        .expect("builds against the mock server");

    let mut stream = provider
        .complete(
            "claude-test",
            CompletionRequest::new(vec![Message::user("hi")]),
        )
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
            grizzly_agent_core::CompletionEvent::Usage(grizzly_agent_core::Usage {
                input_tokens: None,
                output_tokens: Some(1),
            }),
            grizzly_agent_core::CompletionEvent::Finished {
                stop_reason: grizzly_agent_core::StopReason::EndOfTurn,
                raw_stop_reason: "end_turn".to_owned(),
                model: String::new(),
            },
        ]
    );
}

#[tokio::test]
async fn a_non_success_status_fails_before_the_stream_opens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_string("rate limited")
                .insert_header("retry-after", "2"),
        )
        .mount(&server)
        .await;

    let provider = AnthropicProvider::builder("test-key")
        .base_url(server.uri())
        .build()
        .expect("builds against the mock server");

    let outcome = provider
        .complete(
            "claude-test",
            CompletionRequest::new(vec![Message::user("hi")]),
        )
        .await;
    let Err(failure) = outcome else {
        panic!("a 429 must fail before any stream is returned");
    };
    let grizzly_agent_core::ProviderFailure::Status {
        status,
        retry_after,
        ..
    } = failure
    else {
        panic!("expected a Status failure");
    };
    assert_eq!(status, 429);
    assert_eq!(retry_after, Some(std::time::Duration::from_secs(2)));
}

#[tokio::test]
async fn one_provider_instance_serves_two_models_and_each_call_names_its_own_on_the_wire() {
    let server = MockServer::start().await;
    let finished = concat!(
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n",
        "data: {\"type\":\"message_stop\"}\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-a"})))
        .respond_with(ResponseTemplate::new(200).set_body_raw(finished, "text/event-stream"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-b"})))
        .respond_with(ResponseTemplate::new(200).set_body_raw(finished, "text/event-stream"))
        .mount(&server)
        .await;

    let provider = AnthropicProvider::builder("test-key")
        .base_url(server.uri())
        .build()
        .expect("builds against the mock server");

    for model in ["claude-a", "claude-b"] {
        let mut stream = provider
            .complete(model, CompletionRequest::new(vec![Message::user("hi")]))
            .await
            .unwrap_or_else(|_| panic!("the mock matching {model} on the wire must answer"));
        while stream.next().await.is_some() {}
    }
}
