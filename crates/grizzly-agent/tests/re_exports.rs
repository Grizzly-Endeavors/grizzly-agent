//! Every item the facade re-exports, used through `grizzly_agent::` the way
//! a consumer would, so a missing re-export is a compile failure here rather
//! than a surprise in a downstream crate.
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use grizzly_agent::{
    Content, DuplicateToolName, Message, NoParams, ProviderFailure, Role, StopRequest, ToolContext,
    ToolDefinition, ToolFailure, ToolHandler, ToolResult, ToolSet, ToolSpec, ToolUse, TurnFailure,
    TypedToolHandler,
};
use std::borrow::Cow;

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

#[derive(serde::Deserialize)]
struct GreetParams {
    name: String,
}

struct Greet;

impl ToolDefinition for Greet {
    type Params = GreetParams;

    fn spec() -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("greet"),
            description: Cow::Borrowed("greets someone by name"),
            parameters: serde_json::json!({"type": "object"}),
        }
    }
}

struct EchoTool;

#[async_trait::async_trait]
impl ToolHandler for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Cow::Owned("echo".to_owned()),
            description: Cow::Owned("echoes its arguments".to_owned()),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn call(
        &self,
        arguments: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<String, ToolFailure> {
        Ok(arguments.to_string())
    }
}

#[tokio::test]
async fn the_tool_model_is_reachable_through_the_facade() {
    let mut context = ToolContext::new(tokio_util::sync::CancellationToken::new());

    assert!(
        context.request_stop("handing off", "needs a human"),
        "the facade must re-export ToolContext with a usable stop-request API"
    );
    assert_eq!(
        context.take_stop_request(),
        Some(StopRequest {
            reply: "handing off".to_owned(),
            reason: "needs a human".to_owned(),
        }),
        "the facade must re-export StopRequest"
    );

    let no_params: Result<NoParams, _> = serde_json::from_str("{}");
    assert!(
        no_params.is_ok(),
        "the facade must re-export NoParams accepting an empty object"
    );

    let typed_handler =
        TypedToolHandler::<Greet, _>::new(|params: GreetParams, _ctx: &ToolContext| async move {
            Ok(format!("hi {}", params.name))
        });

    let tools = ToolSet::new([
        Box::new(typed_handler) as Box<dyn ToolHandler>,
        Box::new(EchoTool) as Box<dyn ToolHandler>,
    ])
    .expect("the facade must re-export a working ToolSet constructor");

    let dispatch_context = ToolContext::new(tokio_util::sync::CancellationToken::new());
    let greeted = tools
        .dispatch(
            "greet",
            serde_json::json!({"name": "bear"}),
            &dispatch_context,
        )
        .await
        .expect("the typed adapter must dispatch through the facade's ToolSet");
    assert_eq!(greeted, "hi bear");

    let duplicate = ToolSet::new([
        Box::new(EchoTool) as Box<dyn ToolHandler>,
        Box::new(EchoTool) as Box<dyn ToolHandler>,
    ]);
    assert!(
        matches!(duplicate, Err(DuplicateToolName { name }) if name == "echo"),
        "the facade must re-export DuplicateToolName"
    );

    let unknown = tools
        .dispatch("missing", serde_json::Value::Null, &dispatch_context)
        .await;
    assert!(
        matches!(unknown, Err(ToolFailure::Unknown { name, .. }) if name == "missing"),
        "an unknown dispatch through the facade must still report ToolFailure::Unknown"
    );
}
