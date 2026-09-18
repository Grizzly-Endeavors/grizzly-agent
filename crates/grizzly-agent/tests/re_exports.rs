//! Every item the facade re-exports, used through `grizzly_agent::` the way
//! a consumer would, so a missing re-export is a compile failure here rather
//! than a surprise in a downstream crate.
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use grizzly_agent::{
    Content, Message, ProviderFailure, Role, ToolFailure, ToolResult, ToolUse, TurnFailure,
};

#[test]
fn conversation_types_are_reachable_through_the_facade() {
    let message = Message {
        role: Role::User,
        content: vec![
            Content::Text("hello".to_owned()),
            Content::Reasoning {
                text: "thinking".to_owned(),
                signature: None,
            },
            Content::ToolUse(ToolUse {
                id: "call-1".to_owned(),
                name: "read_file".to_owned(),
                input: serde_json::json!({}),
            }),
            Content::ToolResult(ToolResult {
                tool_use_id: "call-1".to_owned(),
                content: "ok".to_owned(),
                is_error: false,
            }),
        ],
    };

    assert_eq!(message.role, Role::User, "the facade must re-export Role");
    assert_eq!(
        message.content.len(),
        4,
        "every Content variant must construct through the facade"
    );
}

#[test]
fn error_types_are_reachable_through_the_facade() {
    let provider_failure = ProviderFailure::InvalidRequest("bad request".to_owned());
    let tool_failure = ToolFailure::Unknown {
        name: "missing".to_owned(),
        available: Vec::new(),
    };
    let turn_failure = TurnFailure::from(provider_failure);

    assert!(
        matches!(turn_failure, TurnFailure::Provider(_)),
        "the facade must re-export TurnFailure and ProviderFailure together"
    );
    assert!(
        tool_failure.into_result("call-1").is_error,
        "the facade must re-export ToolFailure"
    );
}
