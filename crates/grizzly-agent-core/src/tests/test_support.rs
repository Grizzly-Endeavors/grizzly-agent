use futures_util::StreamExt;

use super::*;
use crate::accumulator::CompletionAccumulator;
use crate::completion::{StopReason, Usage};
use crate::message::{Message, ToolUse};

fn sample_completion() -> Completion {
    Completion {
        content: vec![
            Content::Text("here you go: ".to_owned()),
            Content::ToolUse(ToolUse {
                id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::json!({"path": "a.txt"}),
            }),
        ],
        usage: Usage {
            input_tokens: Some(12),
            output_tokens: Some(4),
        },
        stop_reason: StopReason::ToolUse,
        raw_stop_reason: "tool_use".to_owned(),
        model: "test-model".to_owned(),
    }
}

#[tokio::test]
async fn a_completion_response_replays_as_a_well_formed_event_sequence() {
    let provider = ScriptedProvider::new(vec![ScriptedResponse::Completion(sample_completion())]);

    let mut stream = provider
        .complete(
            "test-model",
            CompletionRequest::new(vec![Message::user("hi")]),
        )
        .await
        .expect("the first scripted call must succeed");

    let mut accumulator = CompletionAccumulator::new();
    while let Some(event) = stream.next().await {
        accumulator.push(event.expect("the scripted sequence must be well-formed"));
    }
    let replayed = accumulator
        .finish()
        .expect("a well-formed sequence must fold");

    assert_eq!(
        replayed,
        sample_completion(),
        "the accumulator must reconstruct the scripted completion"
    );
}

#[tokio::test]
async fn a_pre_stream_failure_is_returned_before_any_stream_opens() {
    let provider = ScriptedProvider::new(vec![ScriptedResponse::PreStreamFailure(
        ProviderFailure::Configuration("no credentials".to_owned()),
    )]);

    let error = provider
        .complete(
            "test-model",
            CompletionRequest::new(vec![Message::user("hi")]),
        )
        .await
        .err()
        .expect("a scripted pre-stream failure must surface immediately");

    assert!(matches!(error, ProviderFailure::Configuration(_)));
}

#[tokio::test]
async fn requests_are_recorded_in_call_order() {
    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::Completion(sample_completion()),
        ScriptedResponse::Completion(sample_completion()),
    ]);

    let _ = provider
        .complete(
            "model-a",
            CompletionRequest::new(vec![Message::user("first")]),
        )
        .await
        .expect("first call");
    let _ = provider
        .complete(
            "model-b",
            CompletionRequest::new(vec![Message::user("second")]),
        )
        .await
        .expect("second call");

    let recorded = provider.requests();
    assert_eq!(recorded.len(), 2);
    let first_request = recorded.first().expect("the first recorded request");
    let second_request = recorded.get(1).expect("the second recorded request");
    assert_eq!(
        first_request
            .messages
            .first()
            .expect("the first request's message")
            .text_content(),
        "first"
    );
    assert_eq!(
        second_request
            .messages
            .first()
            .expect("the second request's message")
            .text_content(),
        "second"
    );

    assert_eq!(
        provider.model_ids(),
        vec!["model-a".to_owned(), "model-b".to_owned()],
        "the model id passed to each call must be recorded index-aligned with requests"
    );
}

#[tokio::test]
async fn exhausting_the_script_fails_non_retryably_and_names_the_call_count() {
    let provider = ScriptedProvider::new(vec![ScriptedResponse::Completion(sample_completion())]);

    let _ = provider
        .complete(
            "test-model",
            CompletionRequest::new(vec![Message::user("hi")]),
        )
        .await
        .expect("the scripted call must succeed");
    let error = provider
        .complete(
            "test-model",
            CompletionRequest::new(vec![Message::user("hi")]),
        )
        .await
        .err()
        .expect("a second call with an empty queue must fail");

    assert!(
        matches!(error, ProviderFailure::Configuration(ref message) if message.contains('1')),
        "the failure must name how many calls were served, got {error}"
    );
    assert!(
        !error.is_retryable(),
        "an exhausted script must not be retryable"
    );
}
