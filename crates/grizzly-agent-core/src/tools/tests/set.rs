use super::*;
use crate::tools::context::ToolContext;
use crate::tools::handler::ToolHandler;

/// A hand-written runtime tool, built with an owned wire name rather than a
/// generated static — the shape a consumer wraps around a discovered or
/// configured tool (skills, MCP, config).
struct EchoTool {
    label: String,
}

#[async_trait::async_trait]
impl ToolHandler for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: std::borrow::Cow::Owned(format!("echo_{}", self.label)),
            description: std::borrow::Cow::Owned(format!(
                "echoes arguments back, labeled {}",
                self.label
            )),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn call(&self, arguments: Value, _context: &ToolContext) -> Result<String, ToolFailure> {
        Ok(arguments.to_string())
    }
}

fn context() -> ToolContext {
    ToolContext::new(tokio_util::sync::CancellationToken::new())
}

#[tokio::test]
async fn a_hand_written_runtime_tool_registers_and_dispatches() {
    let tools = ToolSet::new([Box::new(EchoTool {
        label: "one".to_owned(),
    }) as Box<dyn ToolHandler>])
    .expect("a single tool cannot collide with itself");
    let ctx = context();

    let result = tools
        .dispatch("echo_one", serde_json::json!({"a": 1}), &ctx)
        .await;

    assert_eq!(
        result.expect("the registered tool must dispatch"),
        serde_json::json!({"a": 1}).to_string()
    );
}

#[test]
fn duplicate_registration_fails_at_construction() {
    let result = ToolSet::new([
        Box::new(EchoTool {
            label: "dup".to_owned(),
        }) as Box<dyn ToolHandler>,
        Box::new(EchoTool {
            label: "dup".to_owned(),
        }) as Box<dyn ToolHandler>,
    ]);

    match result {
        Err(DuplicateToolName { name }) => assert_eq!(name, "echo_dup"),
        Ok(_) => panic!("two tools sharing a wire name must be rejected"),
    }
}

#[tokio::test]
async fn unknown_dispatch_lists_registered_names() {
    let tools = ToolSet::new([
        Box::new(EchoTool {
            label: "one".to_owned(),
        }) as Box<dyn ToolHandler>,
        Box::new(EchoTool {
            label: "two".to_owned(),
        }) as Box<dyn ToolHandler>,
    ])
    .expect("distinct labels cannot collide");
    let ctx = context();

    let result = tools
        .dispatch("missing_tool", serde_json::Value::Null, &ctx)
        .await;

    match result {
        Err(ToolFailure::Unknown { name, available }) => {
            assert_eq!(name, "missing_tool");
            assert_eq!(
                available,
                vec!["echo_one".to_owned(), "echo_two".to_owned()]
            );
        }
        other => panic!("expected ToolFailure::Unknown, got {other:?}"),
    }
}

#[test]
fn specs_and_names_preserve_registration_order() {
    let tools = ToolSet::new([
        Box::new(EchoTool {
            label: "b".to_owned(),
        }) as Box<dyn ToolHandler>,
        Box::new(EchoTool {
            label: "a".to_owned(),
        }) as Box<dyn ToolHandler>,
    ])
    .expect("distinct labels cannot collide");

    assert_eq!(
        tools.names(),
        vec!["echo_b".to_owned(), "echo_a".to_owned()]
    );
    assert_eq!(
        tools
            .specs()
            .iter()
            .map(|spec| spec.name.as_ref())
            .collect::<Vec<_>>(),
        vec!["echo_b", "echo_a"]
    );
}
