use super::*;
use crate::tools::spec::ToolDefinition;
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Debug, Deserialize)]
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
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
            }),
        }
    }
}

fn context() -> ToolContext {
    ToolContext::new(tokio_util::sync::CancellationToken::new())
}

#[tokio::test]
async fn the_adapter_parses_valid_arguments_and_calls_the_function() {
    let handler =
        TypedToolHandler::<Greet, _>::new(|params: GreetParams, _ctx: &ToolContext| async move {
            Ok(format!("hello, {}", params.name))
        });
    let ctx = context();

    let result = handler
        .call(serde_json::json!({"name": "bear"}), &ctx)
        .await;

    assert_eq!(result.expect("valid arguments must succeed"), "hello, bear");
}

#[tokio::test]
async fn the_adapter_turns_a_parse_failure_into_invalid_arguments_for_the_model() {
    let handler =
        TypedToolHandler::<Greet, _>::new(|params: GreetParams, _ctx: &ToolContext| async move {
            Ok(format!("hello, {}", params.name))
        });
    let ctx = context();

    let result = handler.call(serde_json::json!({"name": 5}), &ctx).await;

    match result {
        Err(ToolFailure::InvalidArguments { name, reason }) => {
            assert_eq!(name, "greet");
            assert!(
                !reason.is_empty(),
                "the reason must explain the mismatch so the model can correct it"
            );
        }
        other => panic!("expected InvalidArguments, got {other:?}"),
    }
}

#[tokio::test]
async fn the_adapter_reports_the_definitions_spec() {
    let handler =
        TypedToolHandler::<Greet, _>::new(|params: GreetParams, _ctx: &ToolContext| async move {
            Ok(params.name)
        });

    let spec = handler.spec();

    assert_eq!(spec.name.as_ref(), "greet");
}
