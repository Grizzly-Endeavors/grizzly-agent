use std::borrow::Cow;

use super::*;
use crate::message::{ToolResult, ToolUse};
use crate::tools::ToolSpec;

#[test]
fn new_defaults_to_no_advertised_tools() {
    let request = CompletionRequest::new(vec![Message::user("hi")]);

    assert!(
        request.tools.is_empty(),
        "a request built with `new` must advertise no tools until the caller sets some"
    );
}

#[test]
fn tools_round_trip_on_the_request() {
    let mut request = CompletionRequest::new(vec![Message::user("hi")]);
    request.tools.push(ToolSpec {
        name: Cow::Borrowed("read_file"),
        description: Cow::Borrowed("reads a file"),
        parameters: serde_json::json!({"type": "object"}),
    });

    assert_eq!(
        request.tools.len(),
        1,
        "a tool spec set on the request must be kept"
    );
    let spec = request.tools.first().expect("the tool spec just pushed");
    assert_eq!(spec.name, "read_file");
}

fn assistant_tool_use(id: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: vec![Content::ToolUse(ToolUse {
            id: id.to_owned(),
            name: "read_file".to_owned(),
            input: serde_json::json!({}),
        })],
    }
}

fn user_tool_result(id: &str) -> Message {
    Message {
        role: Role::User,
        content: vec![Content::ToolResult(ToolResult {
            tool_use_id: id.to_owned(),
            content: "ok".to_owned(),
            is_error: false,
        })],
    }
}

#[test]
fn accepts_a_well_formed_conversation() {
    let request = CompletionRequest::new(vec![
        Message::system("be terse"),
        Message::user("read the file"),
        assistant_tool_use("call-1"),
        user_tool_result("call-1"),
        Message::assistant("done"),
    ]);

    assert!(request.validate().is_ok(), "a valid conversation must pass");
}

#[test]
fn rejects_a_system_message_after_the_head_run() {
    let request = CompletionRequest::new(vec![Message::user("hi"), Message::system("late")]);

    let error = request
        .validate()
        .expect_err("a late system message must be rejected");

    assert!(
        matches!(error, ProviderFailure::InvalidRequest(ref rule) if rule.contains("contiguous run at the head")),
        "the error must name the head-run rule, got {error}"
    );
}

#[test]
fn rejects_a_system_message_with_non_text_content() {
    let request = CompletionRequest::new(vec![Message {
        role: Role::System,
        content: vec![Content::ToolUse(ToolUse {
            id: "call-1".to_owned(),
            name: "read_file".to_owned(),
            input: serde_json::json!({}),
        })],
    }]);

    let error = request
        .validate()
        .expect_err("a non-text system message must be rejected");

    assert!(
        matches!(error, ProviderFailure::InvalidRequest(ref rule) if rule.contains("only text blocks")),
        "the error must name the text-only rule, got {error}"
    );
}

#[test]
fn rejects_tool_use_on_a_user_message() {
    let request = CompletionRequest::new(vec![Message {
        role: Role::User,
        content: vec![Content::ToolUse(ToolUse {
            id: "call-1".to_owned(),
            name: "read_file".to_owned(),
            input: serde_json::json!({}),
        })],
    }]);

    let error = request
        .validate()
        .expect_err("tool-use on a user message must be rejected");

    assert!(
        matches!(error, ProviderFailure::InvalidRequest(ref rule) if rule.contains("assistant messages")),
        "the error must name the assistant-only rule, got {error}"
    );
}

#[test]
fn rejects_reasoning_on_a_user_message() {
    let request = CompletionRequest::new(vec![Message {
        role: Role::User,
        content: vec![Content::Reasoning {
            text: "thinking".to_owned(),
            signature: None,
        }],
    }]);

    let error = request
        .validate()
        .expect_err("reasoning on a user message must be rejected");

    assert!(
        matches!(error, ProviderFailure::InvalidRequest(ref rule) if rule.contains("assistant messages")),
        "the error must name the assistant-only rule, got {error}"
    );
}

#[test]
fn rejects_a_tool_result_on_an_assistant_message() {
    let request = CompletionRequest::new(vec![
        assistant_tool_use("call-1"),
        Message {
            role: Role::Assistant,
            content: vec![Content::ToolResult(ToolResult {
                tool_use_id: "call-1".to_owned(),
                content: "ok".to_owned(),
                is_error: false,
            })],
        },
    ]);

    let error = request
        .validate()
        .expect_err("a tool result on an assistant message must be rejected");

    assert!(
        matches!(error, ProviderFailure::InvalidRequest(ref rule) if rule.contains("user messages")),
        "the error must name the user-only rule, got {error}"
    );
}

#[test]
fn rejects_a_tool_result_with_no_matching_tool_use() {
    let request = CompletionRequest::new(vec![user_tool_result("call-1")]);

    let error = request
        .validate()
        .expect_err("an unanswered tool result must be rejected");

    assert!(
        matches!(error, ProviderFailure::InvalidRequest(ref rule) if rule.contains("answer a tool use")),
        "the error must name the answers-a-tool-use rule, got {error}"
    );
}

#[test]
fn allows_a_user_message_mixing_tool_results_and_text() {
    let request = CompletionRequest::new(vec![
        assistant_tool_use("call-1"),
        Message {
            role: Role::User,
            content: vec![
                Content::ToolResult(ToolResult {
                    tool_use_id: "call-1".to_owned(),
                    content: "ok".to_owned(),
                    is_error: false,
                }),
                Content::Text("anything else?".to_owned()),
            ],
        },
    ]);

    assert!(
        request.validate().is_ok(),
        "a user message may mix tool results and text"
    );
}
