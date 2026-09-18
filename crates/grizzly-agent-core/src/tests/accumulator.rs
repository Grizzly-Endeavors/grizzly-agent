use super::*;
use crate::completion::CompletionEvent;

fn finished(stop_reason: StopReason) -> CompletionEvent {
    CompletionEvent::Finished {
        stop_reason,
        raw_stop_reason: "raw".to_owned(),
        model: "test-model".to_owned(),
    }
}

#[test]
fn text_deltas_of_the_same_kind_merge_into_one_block() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::TextDelta("hello ".to_owned()));
    accumulator.push(CompletionEvent::TextDelta("world".to_owned()));
    accumulator.push(finished(StopReason::EndOfTurn));

    let completion = accumulator.finish().expect("a finished stream must fold");

    assert_eq!(
        completion.content,
        vec![Content::Text("hello world".to_owned())],
        "consecutive text deltas must merge into a single block"
    );
}

#[test]
fn a_delta_of_a_different_kind_starts_a_new_block_and_order_follows_the_stream() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::ReasoningDelta("thinking".to_owned()));
    accumulator.push(CompletionEvent::TextDelta("reply".to_owned()));
    accumulator.push(CompletionEvent::ReasoningDelta("more thinking".to_owned()));
    accumulator.push(finished(StopReason::EndOfTurn));

    let completion = accumulator.finish().expect("a finished stream must fold");

    assert_eq!(
        completion.content,
        vec![
            Content::Reasoning {
                text: "thinking".to_owned(),
                signature: None,
            },
            Content::Text("reply".to_owned()),
            Content::Reasoning {
                text: "more thinking".to_owned(),
                signature: None,
            },
        ],
        "block order in the reassembled message must follow the stream"
    );
}

#[test]
fn interleaved_tool_call_argument_fragments_reassemble_by_id() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::ToolUseStart {
        id: "call-1".to_owned(),
        name: "read_file".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseStart {
        id: "call-2".to_owned(),
        name: "write_file".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseArgumentsDelta {
        id: "call-1".to_owned(),
        fragment: "{\"pat".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseArgumentsDelta {
        id: "call-2".to_owned(),
        fragment: "{\"path\":".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseArgumentsDelta {
        id: "call-1".to_owned(),
        fragment: "h\":\"a.txt\"}".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseArgumentsDelta {
        id: "call-2".to_owned(),
        fragment: "\"b.txt\"}".to_owned(),
    });
    accumulator.push(finished(StopReason::ToolUse));

    let completion = accumulator
        .finish()
        .expect("interleaved fragments must reassemble");

    let Content::ToolUse(first) = completion.content.first().expect("first block") else {
        panic!("expected a tool-use block first");
    };
    let Content::ToolUse(second) = completion.content.get(1).expect("second block") else {
        panic!("expected a tool-use block second");
    };
    assert_eq!(first.id, "call-1");
    assert_eq!(first.input, serde_json::json!({"path": "a.txt"}));
    assert_eq!(second.id, "call-2");
    assert_eq!(second.input, serde_json::json!({"path": "b.txt"}));
}

#[test]
fn unparseable_arguments_are_a_decode_failure() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::ToolUseStart {
        id: "call-1".to_owned(),
        name: "read_file".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseArgumentsDelta {
        id: "call-1".to_owned(),
        fragment: "{not json".to_owned(),
    });
    accumulator.push(finished(StopReason::ToolUse));

    let error = accumulator
        .finish()
        .expect_err("unparseable arguments must fail decode");

    assert!(
        matches!(error, ProviderFailure::Decode { .. }),
        "unparseable tool arguments must be a decode-class failure, got {error}"
    );
}

#[test]
fn max_tokens_drops_an_incomplete_tool_use_block_and_keeps_the_rest() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::TextDelta("here goes".to_owned()));
    accumulator.push(CompletionEvent::ToolUseStart {
        id: "call-1".to_owned(),
        name: "read_file".to_owned(),
    });
    accumulator.push(CompletionEvent::ToolUseArgumentsDelta {
        id: "call-1".to_owned(),
        fragment: "{\"path\": \"cut off".to_owned(),
    });
    accumulator.push(finished(StopReason::MaxTokens));

    let completion = accumulator
        .finish()
        .expect("a truncated tool call under max-tokens must not fail");

    assert_eq!(
        completion.content,
        vec![Content::Text("here goes".to_owned())],
        "the incomplete tool-use block must be dropped, keeping the rest of the completion"
    );
}

#[test]
fn usage_events_merge_field_by_field_in_arrival_order() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::Usage(Usage {
        input_tokens: Some(10),
        output_tokens: None,
    }));
    accumulator.push(CompletionEvent::Usage(Usage {
        input_tokens: None,
        output_tokens: Some(20),
    }));
    accumulator.push(finished(StopReason::EndOfTurn));

    let completion = accumulator.finish().expect("a finished stream must fold");

    assert_eq!(
        completion.usage,
        Usage {
            input_tokens: Some(10),
            output_tokens: Some(20),
        },
        "usage fields must merge across events rather than the last event replacing the whole struct"
    );
}

#[test]
fn a_stream_that_never_finishes_fails_retryably() {
    let mut accumulator = CompletionAccumulator::new();
    accumulator.push(CompletionEvent::TextDelta("partial".to_owned()));

    let error = accumulator
        .finish()
        .expect_err("a stream with no finished event must fail");

    assert!(
        error.is_retryable(),
        "a stream ending without a finished event must fail retryably, got {error}"
    );
}
