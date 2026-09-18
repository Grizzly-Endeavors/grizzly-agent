use grizzly_agent_core::{
    Content, Message, ResponseFormat, Role, StopReason, ToolResult, ToolSpec, ToolUse,
};

use super::*;

fn request_with(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest::new(messages)
}

fn wire_json(request: &CompletionRequest, model: &str) -> serde_json::Value {
    serde_json::to_value(build_request(request, model)).expect("the wire body always serializes")
}

/// A JSON-Pointer lookup (`/messages/0/role`), so tests never index a
/// `Vec`/`Value` directly — `indexing_slicing` is denied even in tests.
fn get<'a>(value: &'a serde_json::Value, pointer: &str) -> &'a serde_json::Value {
    value
        .pointer(pointer)
        .unwrap_or_else(|| panic!("expected `{pointer}` in {value}"))
}

fn array_len(value: &serde_json::Value, pointer: &str) -> usize {
    get(value, pointer)
        .as_array()
        .unwrap_or_else(|| panic!("expected `{pointer}` to be an array in {value}"))
        .len()
}

#[test]
fn every_request_streams_and_asks_for_usage() {
    let value = wire_json(&request_with(vec![Message::user("hi")]), "gpt-test");
    assert_eq!(get(&value, "/stream"), &serde_json::json!(true));
    assert_eq!(
        get(&value, "/stream_options"),
        &serde_json::json!({ "include_usage": true })
    );
    assert_eq!(get(&value, "/model"), &serde_json::json!("gpt-test"));
}

#[test]
fn multiple_head_system_messages_pass_through_as_separate_system_messages() {
    let request = request_with(vec![
        Message::system("be terse"),
        Message::system("never apologize"),
        Message::user("hi"),
    ]);
    let value = wire_json(&request, "gpt-test");

    assert_eq!(
        array_len(&value, "/messages"),
        3,
        "no joining on this wire — each passes through"
    );
    assert_eq!(
        get(&value, "/messages/0/role"),
        &serde_json::json!("system")
    );
    assert_eq!(
        get(&value, "/messages/0/content"),
        &serde_json::json!("be terse")
    );
    assert_eq!(
        get(&value, "/messages/1/role"),
        &serde_json::json!("system")
    );
    assert_eq!(
        get(&value, "/messages/1/content"),
        &serde_json::json!("never apologize")
    );
}

#[test]
fn a_mixed_tool_result_and_text_user_message_becomes_tool_messages_then_one_user_message() {
    let request = request_with(vec![
        Message::assistant("checking"),
        Message {
            role: Role::User,
            content: vec![
                Content::ToolResult(ToolResult {
                    tool_use_id: "call-1".to_owned(),
                    content: "42".to_owned(),
                    is_error: false,
                }),
                Content::Text("thanks, and one more thing".to_owned()),
                Content::ToolResult(ToolResult {
                    tool_use_id: "call-2".to_owned(),
                    content: "ok".to_owned(),
                    is_error: false,
                }),
            ],
        },
    ]);
    let value = wire_json(&request, "gpt-test");

    // assistant message, then both tool results, then one trailing user message
    assert_eq!(array_len(&value, "/messages"), 4);
    assert_eq!(get(&value, "/messages/1/role"), &serde_json::json!("tool"));
    assert_eq!(
        get(&value, "/messages/1/tool_call_id"),
        &serde_json::json!("call-1")
    );
    assert_eq!(get(&value, "/messages/2/role"), &serde_json::json!("tool"));
    assert_eq!(
        get(&value, "/messages/2/tool_call_id"),
        &serde_json::json!("call-2")
    );
    assert_eq!(get(&value, "/messages/3/role"), &serde_json::json!("user"));
    assert_eq!(
        get(&value, "/messages/3/content"),
        &serde_json::json!("thanks, and one more thing")
    );
}

#[test]
fn a_user_message_with_only_tool_results_carries_no_trailing_user_message() {
    let request = request_with(vec![
        Message::assistant("checking"),
        Message::tool_results(vec![ToolResult {
            tool_use_id: "call-1".to_owned(),
            content: "42".to_owned(),
            is_error: false,
        }]),
    ]);
    let value = wire_json(&request, "gpt-test");

    assert_eq!(
        array_len(&value, "/messages"),
        2,
        "no trailing user message with no text"
    );
    assert_eq!(get(&value, "/messages/1/role"), &serde_json::json!("tool"));
}

#[test]
fn reasoning_blocks_are_dropped_from_outgoing_assistant_history() {
    let request = request_with(vec![Message {
        role: Role::Assistant,
        content: vec![
            Content::Reasoning {
                text: "scratch work".to_owned(),
                signature: None,
            },
            Content::Text("the answer is 4".to_owned()),
        ],
    }]);
    let value = wire_json(&request, "gpt-test");

    assert_eq!(array_len(&value, "/messages"), 1);
    assert_eq!(
        get(&value, "/messages/0/content"),
        &serde_json::json!("the answer is 4")
    );
    assert!(
        value.to_string().contains("the answer is 4")
            && !value.to_string().contains("scratch work"),
        "reasoning text must never reach the wire"
    );
}

#[test]
fn an_assistant_tool_use_becomes_a_wire_tool_call() {
    let request = request_with(vec![Message {
        role: Role::Assistant,
        content: vec![Content::ToolUse(ToolUse {
            id: "call-1".to_owned(),
            name: "bash".to_owned(),
            input: serde_json::json!({"command": "ls"}),
        })],
    }]);
    let value = wire_json(&request, "gpt-test");

    assert_eq!(array_len(&value, "/messages/0/tool_calls"), 1);
    assert_eq!(
        get(&value, "/messages/0/tool_calls/0/id"),
        &serde_json::json!("call-1")
    );
    assert_eq!(
        get(&value, "/messages/0/tool_calls/0/function/name"),
        &serde_json::json!("bash")
    );
    assert_eq!(
        get(&value, "/messages/0/tool_calls/0/function/arguments"),
        &serde_json::json!(r#"{"command":"ls"}"#)
    );
}

#[test]
fn tool_specs_map_to_function_tools() {
    let mut request = request_with(vec![Message::user("hi")]);
    request.tools = vec![ToolSpec {
        name: "bash".into(),
        description: "run a shell command".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];
    let value = wire_json(&request, "gpt-test");

    assert_eq!(array_len(&value, "/tools"), 1);
    assert_eq!(get(&value, "/tools/0/type"), &serde_json::json!("function"));
    assert_eq!(
        get(&value, "/tools/0/function/name"),
        &serde_json::json!("bash")
    );
}

#[test]
fn a_response_format_becomes_strict_json_schema() {
    let mut request = request_with(vec![Message::user("hi")]);
    request.response_format = Some(ResponseFormat {
        name: "answer".to_owned(),
        schema: serde_json::json!({"type": "object"}),
    });
    let value = wire_json(&request, "gpt-test");

    assert_eq!(
        get(&value, "/response_format/type"),
        &serde_json::json!("json_schema")
    );
    assert_eq!(
        get(&value, "/response_format/json_schema/strict"),
        &serde_json::json!(true)
    );
    assert_eq!(
        get(&value, "/response_format/json_schema/name"),
        &serde_json::json!("answer")
    );
}

// --- Incoming: EventTranslator ----------------------------------------------

fn chunk(json: serde_json::Value) -> ChunkWire {
    serde_json::from_value(json).expect("test fixture must parse")
}

#[test]
fn a_text_delta_becomes_a_text_delta_event() {
    let mut translator = EventTranslator::new();
    let events = translator.absorb(chunk(serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": "hello"}}]
    })));
    assert_eq!(events, vec![CompletionEvent::TextDelta("hello".to_owned())]);
}

#[test]
fn each_reasoning_spelling_maps_to_a_reasoning_delta() {
    for field in ["reasoning_content", "reasoning", "thinking"] {
        let mut translator = EventTranslator::new();
        let events = translator.absorb(chunk(serde_json::json!({
            "choices": [{"index": 0, "delta": {field: "thinking..."}}]
        })));
        assert_eq!(
            events,
            vec![CompletionEvent::ReasoningDelta("thinking...".to_owned())],
            "the `{field}` spelling must map to a reasoning delta"
        );
    }
}

#[test]
fn tool_call_fragments_reassemble_into_id_addressed_events() {
    let mut translator = EventTranslator::new();
    let mut events = translator.absorb(chunk(serde_json::json!({
        "choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "id": "call_1", "function": {"name": "bash", "arguments": "{\"cmd"}}
        ]}}]
    })));
    events.extend(translator.absorb(chunk(serde_json::json!({
        "choices": [{"index": 0, "delta": {"tool_calls": [
            {"index": 0, "function": {"arguments": "\":\"ls\"}"}}
        ]}}]
    }))));

    assert_eq!(
        events,
        vec![
            CompletionEvent::ToolUseStart {
                id: "call_1".to_owned(),
                name: "bash".to_owned(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "call_1".to_owned(),
                fragment: "{\"cmd".to_owned(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "call_1".to_owned(),
                fragment: "\":\"ls\"}".to_owned(),
            },
        ]
    );
}

#[test]
fn a_finish_reason_is_buffered_until_finalize() {
    let mut translator = EventTranslator::new();
    let events = translator.absorb(chunk(serde_json::json!({
        "model": "gpt-test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    })));
    assert!(
        events.is_empty(),
        "the finish event is held back, not emitted with the chunk that named it"
    );

    let finished = translator.finalize();
    assert_eq!(
        finished,
        Some(CompletionEvent::Finished {
            stop_reason: StopReason::EndOfTurn,
            raw_stop_reason: "stop".to_owned(),
            model: "gpt-test".to_owned(),
        })
    );
}

#[test]
fn finalize_with_no_finish_reason_ever_seen_is_none() {
    let mut translator = EventTranslator::new();
    translator.absorb(chunk(serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": "half a "}}]
    })));
    assert_eq!(
        translator.finalize(),
        None,
        "no finish_reason was ever seen, so the caller must fail retryably instead"
    );
}

#[test]
fn stop_reasons_classify_as_documented() {
    for (raw, expected) in [
        ("stop", StopReason::EndOfTurn),
        ("tool_calls", StopReason::ToolUse),
        ("length", StopReason::MaxTokens),
        ("content_filter", StopReason::Other),
    ] {
        let mut translator = EventTranslator::new();
        translator.absorb(chunk(serde_json::json!({
            "model": "gpt-test",
            "choices": [{"index": 0, "delta": {}, "finish_reason": raw}]
        })));
        let Some(CompletionEvent::Finished { stop_reason, .. }) = translator.finalize() else {
            panic!("expected a Finished event");
        };
        assert_eq!(stop_reason, expected, "raw reason `{raw}`");
    }
}

#[test]
fn a_usage_chunk_after_the_finish_reason_is_still_ordered_before_finished() {
    let mut translator = EventTranslator::new();
    let mut events = translator.absorb(chunk(serde_json::json!({
        "model": "gpt-test",
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
    })));
    events.extend(translator.absorb(chunk(serde_json::json!({
        "choices": [],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5}
    }))));
    let finished = translator.finalize().expect("finish_reason was seen");
    events.push(finished);

    let first = events.first().expect("at least the usage event is queued");
    let CompletionEvent::Usage(usage) = first else {
        panic!("expected the usage event first, got {first:?}");
    };
    assert_eq!(usage.input_tokens, Some(10));
    assert!(matches!(
        events.last(),
        Some(CompletionEvent::Finished { .. })
    ));
}

#[test]
fn choices_from_a_secondary_index_are_ignored() {
    let mut translator = EventTranslator::new();
    let events = translator.absorb(chunk(serde_json::json!({
        "choices": [{"index": 1, "delta": {"content": "ignored"}}]
    })));
    assert!(events.is_empty(), "only choice index 0 is read");
}
