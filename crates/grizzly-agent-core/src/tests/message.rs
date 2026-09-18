//! Tests for [`super`].

use super::{Content, Message, Role, ToolResult, ToolUse};

fn tool_use(id: &str, name: &str) -> ToolUse {
    ToolUse {
        id: id.to_owned(),
        name: name.to_owned(),
        input: serde_json::json!({}),
    }
}

#[test]
fn text_content_joins_only_text_blocks() {
    let message = Message {
        role: Role::Assistant,
        content: vec![
            Content::Reasoning {
                text: "deliberating".to_owned(),
                signature: None,
            },
            Content::Text("first".to_owned()),
            Content::ToolUse(tool_use("t1", "read")),
            Content::Text("second".to_owned()),
        ],
    };

    assert_eq!(
        message.text_content(),
        "first\nsecond",
        "reasoning and tool use must not leak into rendered text"
    );
}

#[test]
fn reasoning_is_never_treated_as_text() {
    let message = Message {
        role: Role::Assistant,
        content: vec![Content::Reasoning {
            text: "thinking out loud".to_owned(),
            signature: Some("sig-1".to_owned()),
        }],
    };

    assert_eq!(
        message.text_content(),
        "",
        "a reply that is only reasoning has no text to render"
    );
}

#[test]
fn tool_only_message_has_empty_text_rather_than_none() {
    let message = Message {
        role: Role::Assistant,
        content: vec![Content::ToolUse(tool_use("t1", "read"))],
    };

    assert_eq!(
        message.text_content(),
        "",
        "a tool-only turn is ordinary, not a missing value"
    );
}

#[test]
fn requests_tools_drives_the_loop_stop_condition() {
    let asking = Message {
        role: Role::Assistant,
        content: vec![
            Content::Text("let me look".to_owned()),
            Content::ToolUse(tool_use("t1", "read")),
        ],
    };
    let done = Message::assistant("here is the answer");

    assert!(
        asking.requests_tools(),
        "text alongside a tool call is a preamble, not a final answer"
    );
    assert!(
        !done.requests_tools(),
        "a reply with no tool calls ends the turn"
    );
}

#[test]
fn tool_uses_preserves_order_across_a_batch() {
    let message = Message {
        role: Role::Assistant,
        content: vec![
            Content::ToolUse(tool_use("t1", "read")),
            Content::Text("and then".to_owned()),
            Content::ToolUse(tool_use("t2", "write")),
        ],
    };

    let names: Vec<&str> = message
        .tool_uses()
        .iter()
        .map(|use_| use_.name.as_str())
        .collect();

    assert_eq!(
        names,
        vec!["read", "write"],
        "batched tool calls must stay in the order the model issued them"
    );
}

#[test]
fn tool_results_builds_one_block_per_result() {
    let message = Message::tool_results([
        ToolResult {
            tool_use_id: "t1".to_owned(),
            content: "ok".to_owned(),
            is_error: false,
        },
        ToolResult {
            tool_use_id: "t2".to_owned(),
            content: "no such file".to_owned(),
            is_error: true,
        },
    ]);

    assert_eq!(
        message.role,
        Role::User,
        "the model reads tool output as user-supplied input"
    );
    assert_eq!(message.content.len(), 2, "each result gets its own block");
}

#[test]
fn a_failed_tool_result_is_still_content() {
    let result = ToolResult {
        tool_use_id: "t1".to_owned(),
        content: "permission denied".to_owned(),
        is_error: true,
    };

    let message = Message::tool_results([result]);

    assert_eq!(
        message.text_content(),
        "",
        "tool results are not the assistant's rendered text"
    );
    assert!(
        matches!(
            message.content.first(),
            Some(Content::ToolResult(inner)) if inner.is_error
        ),
        "a failure must survive as a flagged result the model can read, not an error"
    );
}

#[test]
fn roles_serialize_lowercase_for_wire_compatibility() {
    let encoded = serde_json::to_string(&Role::Assistant).expect("role serialization cannot fail");

    assert_eq!(
        encoded, "\"assistant\"",
        "providers expect lowercase role names on the wire"
    );
}
