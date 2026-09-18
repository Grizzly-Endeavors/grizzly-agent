use grizzly_agent_core::{
    Content, Message, ResponseFormat, Role, StopReason, ToolResult, ToolSpec, ToolUse,
};

use super::*;

fn request_with(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest::new(messages)
}

fn wire_json(
    request: &CompletionRequest,
    model: &str,
    default_max_tokens: u32,
) -> serde_json::Value {
    serde_json::to_value(build_request(request, model, default_max_tokens))
        .expect("the wire body always serializes")
}

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
fn every_request_streams_and_carries_max_tokens() {
    let value = wire_json(
        &request_with(vec![Message::user("hi")]),
        "claude-test",
        2048,
    );
    assert_eq!(get(&value, "/stream"), &serde_json::json!(true));
    assert_eq!(get(&value, "/model"), &serde_json::json!("claude-test"));
    assert_eq!(get(&value, "/max_tokens"), &serde_json::json!(2048));
}

#[test]
fn an_unset_max_tokens_falls_back_to_the_provider_default() {
    let mut request = request_with(vec![Message::user("hi")]);
    request.max_tokens = None;
    let value = wire_json(&request, "claude-test", 4096);
    assert_eq!(get(&value, "/max_tokens"), &serde_json::json!(4096));
}

#[test]
fn a_request_supplied_max_tokens_wins_over_the_default() {
    let mut request = request_with(vec![Message::user("hi")]);
    request.max_tokens = Some(64);
    let value = wire_json(&request, "claude-test", 4096);
    assert_eq!(get(&value, "/max_tokens"), &serde_json::json!(64));
}

#[test]
fn multiple_head_system_messages_are_lifted_and_joined_with_a_blank_line() {
    let request = request_with(vec![
        Message::system("be terse"),
        Message::system("never apologize"),
        Message::user("hi"),
    ]);
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(
        get(&value, "/system"),
        &serde_json::json!("be terse\n\nnever apologize"),
        "head system messages join with a blank line into one field"
    );
    assert_eq!(
        array_len(&value, "/messages"),
        1,
        "system messages never appear in the messages array"
    );
}

#[test]
fn no_system_messages_means_no_system_field() {
    let value = wire_json(
        &request_with(vec![Message::user("hi")]),
        "claude-test",
        1024,
    );
    assert!(
        value.pointer("/system").is_none(),
        "the system field is omitted entirely when there is nothing to lift"
    );
}

#[test]
fn a_mixed_tool_result_and_text_user_message_maps_natively_in_one_message() {
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
                Content::Text("thanks".to_owned()),
            ],
        },
    ]);
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(
        array_len(&value, "/messages"),
        2,
        "no restructuring: one core message stays one wire message"
    );
    assert_eq!(array_len(&value, "/messages/1/content"), 2);
    assert_eq!(
        get(&value, "/messages/1/content/0/type"),
        &serde_json::json!("tool_result")
    );
    assert_eq!(
        get(&value, "/messages/1/content/0/tool_use_id"),
        &serde_json::json!("call-1")
    );
    assert_eq!(
        get(&value, "/messages/1/content/1/type"),
        &serde_json::json!("text")
    );
}

#[test]
fn signed_reasoning_round_trips_as_a_thinking_block() {
    let request = request_with(vec![Message {
        role: Role::Assistant,
        content: vec![
            Content::Reasoning {
                text: "scratch work".to_owned(),
                signature: Some("sig-abc".to_owned()),
            },
            Content::Text("the answer is 4".to_owned()),
        ],
    }]);
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(array_len(&value, "/messages/0/content"), 2);
    assert_eq!(
        get(&value, "/messages/0/content/0/type"),
        &serde_json::json!("thinking")
    );
    assert_eq!(
        get(&value, "/messages/0/content/0/thinking"),
        &serde_json::json!("scratch work")
    );
    assert_eq!(
        get(&value, "/messages/0/content/0/signature"),
        &serde_json::json!("sig-abc")
    );
}

#[test]
fn unsigned_reasoning_is_dropped_from_outgoing_history() {
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
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(
        array_len(&value, "/messages/0/content"),
        1,
        "unsigned reasoning must never reach the wire"
    );
    assert_eq!(
        get(&value, "/messages/0/content/0/type"),
        &serde_json::json!("text")
    );
}

#[test]
fn an_assistant_tool_use_maps_to_a_native_tool_use_block() {
    let request = request_with(vec![Message {
        role: Role::Assistant,
        content: vec![Content::ToolUse(ToolUse {
            id: "call-1".to_owned(),
            name: "bash".to_owned(),
            input: serde_json::json!({"command": "ls"}),
        })],
    }]);
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(
        get(&value, "/messages/0/content/0/type"),
        &serde_json::json!("tool_use")
    );
    assert_eq!(
        get(&value, "/messages/0/content/0/id"),
        &serde_json::json!("call-1")
    );
    assert_eq!(
        get(&value, "/messages/0/content/0/input"),
        &serde_json::json!({"command": "ls"}),
        "tool_use input is the JSON object itself, not a serialized string"
    );
}

#[test]
fn tool_specs_map_to_input_schema() {
    let mut request = request_with(vec![Message::user("hi")]);
    request.tools = vec![ToolSpec {
        name: "bash".into(),
        description: "run a shell command".into(),
        parameters: serde_json::json!({"type": "object"}),
    }];
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(array_len(&value, "/tools"), 1);
    assert_eq!(get(&value, "/tools/0/name"), &serde_json::json!("bash"));
    assert_eq!(
        get(&value, "/tools/0/input_schema"),
        &serde_json::json!({"type": "object"})
    );
}

#[test]
fn a_response_format_becomes_output_config_json_schema() {
    let mut request = request_with(vec![Message::user("hi")]);
    request.response_format = Some(ResponseFormat {
        name: "answer".to_owned(),
        schema: serde_json::json!({"type": "object"}),
    });
    let value = wire_json(&request, "claude-test", 1024);

    assert_eq!(
        get(&value, "/output_config/format/type"),
        &serde_json::json!("json_schema")
    );
    assert_eq!(
        get(&value, "/output_config/format/schema"),
        &serde_json::json!({"type": "object"})
    );
}

// --- Incoming: EventTranslator ----------------------------------------------

fn event(json: serde_json::Value) -> StreamEventWire {
    serde_json::from_value(json).expect("test fixture must parse")
}

#[test]
fn message_start_reports_the_model_and_initial_usage() {
    let mut translator = EventTranslator::new();
    let events = translator
        .absorb(event(serde_json::json!({
            "type": "message_start",
            "message": {"model": "claude-test", "usage": {"input_tokens": 12, "output_tokens": 0}}
        })))
        .expect("message_start never carries an error");
    assert_eq!(
        events,
        vec![CompletionEvent::Usage(grizzly_agent_core::Usage {
            input_tokens: Some(12),
            output_tokens: Some(0),
        })]
    );
}

#[test]
fn a_tool_use_block_start_emits_tool_use_start() {
    let mut translator = EventTranslator::new();
    let events = translator
        .absorb(event(serde_json::json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "tool_use", "id": "toolu_1", "name": "bash", "input": {}}
        })))
        .expect("no error");
    assert_eq!(
        events,
        vec![CompletionEvent::ToolUseStart {
            id: "toolu_1".to_owned(),
            name: "bash".to_owned(),
        }]
    );
}

#[test]
fn input_json_deltas_are_addressed_by_block_index_not_a_per_fragment_id() {
    let mut translator = EventTranslator::new();
    translator
        .absorb(event(serde_json::json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "tool_use", "id": "toolu_1", "name": "bash", "input": {}}
        })))
        .expect("no error");
    let events = translator
        .absorb(event(serde_json::json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "input_json_delta", "partial_json": "{\"cmd\":\"ls\"}"}
        })))
        .expect("no error");
    assert_eq!(
        events,
        vec![CompletionEvent::ToolUseArgumentsDelta {
            id: "toolu_1".to_owned(),
            fragment: "{\"cmd\":\"ls\"}".to_owned(),
        }]
    );
}

#[test]
fn thinking_and_signature_deltas_map_to_reasoning_events() {
    let mut translator = EventTranslator::new();
    let mut events = translator
        .absorb(event(serde_json::json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "checking the math"}
        })))
        .expect("no error");
    events.extend(
        translator
            .absorb(event(serde_json::json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "signature_delta", "signature": "sig-abc"}
            })))
            .expect("no error"),
    );

    assert_eq!(
        events,
        vec![
            CompletionEvent::ReasoningDelta("checking the math".to_owned()),
            CompletionEvent::ReasoningSignatureDelta("sig-abc".to_owned()),
        ]
    );
}

#[test]
fn message_delta_buffers_the_finish_reason_and_reports_usage_immediately() {
    let mut translator = EventTranslator::new();
    let events = translator
        .absorb(event(serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 6}
        })))
        .expect("no error");
    assert_eq!(
        events,
        vec![CompletionEvent::Usage(grizzly_agent_core::Usage {
            input_tokens: None,
            output_tokens: Some(6),
        })],
        "usage rides message_delta immediately; only the finish event is held back"
    );
}

#[test]
fn message_stop_releases_the_buffered_finish_event() {
    let mut translator = EventTranslator::new();
    translator
        .absorb(event(serde_json::json!({
            "type": "message_start",
            "message": {"model": "claude-test"}
        })))
        .expect("no error");
    translator
        .absorb(event(serde_json::json!({
            "type": "message_delta",
            "delta": {"stop_reason": "tool_use"},
            "usage": {"output_tokens": 3}
        })))
        .expect("no error");

    let finished = translator.finalize();
    assert_eq!(
        finished,
        Some(CompletionEvent::Finished {
            stop_reason: StopReason::ToolUse,
            raw_stop_reason: "tool_use".to_owned(),
            model: "claude-test".to_owned(),
        })
    );
}

#[test]
fn finalize_with_no_message_delta_ever_seen_is_none() {
    let translator = EventTranslator::new();
    assert_eq!(
        translator.finalize(),
        None,
        "no stop_reason was ever seen, so the caller must fail retryably instead"
    );
}

#[test]
fn stop_reasons_classify_as_documented() {
    for (raw, expected) in [
        ("end_turn", StopReason::EndOfTurn),
        ("tool_use", StopReason::ToolUse),
        ("max_tokens", StopReason::MaxTokens),
        ("pause_turn", StopReason::Other),
        ("refusal", StopReason::Other),
    ] {
        let mut translator = EventTranslator::new();
        translator
            .absorb(event(serde_json::json!({
                "type": "message_delta",
                "delta": {"stop_reason": raw},
                "usage": {"output_tokens": 1}
            })))
            .expect("no error");
        let Some(CompletionEvent::Finished { stop_reason, .. }) = translator.finalize() else {
            panic!("expected a Finished event");
        };
        assert_eq!(stop_reason, expected, "raw reason `{raw}`");
    }
}

#[test]
fn a_ping_event_produces_no_events() {
    let mut translator = EventTranslator::new();
    let events = translator
        .absorb(event(serde_json::json!({"type": "ping"})))
        .expect("ping never carries an error");
    assert!(events.is_empty());
}

#[test]
fn an_unrecognized_event_type_is_ignored_rather_than_failing_to_parse() {
    let mut translator = EventTranslator::new();
    let events = translator
        .absorb(event(serde_json::json!({
            "type": "citations_delta",
            "something": "unexpected"
        })))
        .expect("an unknown event type must not error");
    assert!(events.is_empty());
}

#[test]
fn an_error_event_is_reported_as_an_error() {
    let mut translator = EventTranslator::new();
    let outcome = translator.absorb(event(serde_json::json!({
        "type": "error",
        "error": {"type": "overloaded_error", "message": "Overloaded"}
    })));
    let error = outcome.expect_err("an error event must surface as Err");
    assert_eq!(get(&error, "/type"), &serde_json::json!("overloaded_error"));
}
