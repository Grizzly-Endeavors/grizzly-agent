//! Every item the facade re-exports, used through `grizzly_agent::` the way
//! a consumer would, so a missing re-export is a compile failure here rather
//! than a surprise in a downstream crate.
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use std::sync::Arc;

use futures_util::StreamExt;
use grizzly_agent::{
    Completion, CompletionAccumulator, CompletionEvent, CompletionRequest, CompletionStream,
    Content, DuplicateToolName, Message, Model, ModelBuilder, NoParams, Provider, ProviderFailure,
    ResponseFormat, RetryPolicy, Role, StopReason, StopRequest, ToolContext, ToolDefinition,
    ToolFailure, ToolHandler, ToolResult, ToolSet, ToolSpec, ToolUse, TurnFailure,
    TypedToolHandler, Usage,
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

/// A minimal [`Provider`] so this test exercises the facade's model-call
/// surface without needing the `test-support` feature.
struct StubProvider;

#[async_trait::async_trait]
impl Provider for StubProvider {
    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure> {
        let events: Vec<Result<CompletionEvent, ProviderFailure>> = vec![
            Ok(CompletionEvent::TextDelta("hello".to_owned())),
            Ok(CompletionEvent::Usage(Usage {
                input_tokens: Some(1),
                output_tokens: Some(1),
            })),
            Ok(CompletionEvent::Finished {
                stop_reason: StopReason::EndOfTurn,
                raw_stop_reason: "stop".to_owned(),
                model: "stub-model".to_owned(),
            }),
        ];
        Ok(futures_util::stream::iter(events).boxed())
    }
}

/// Takes and returns a [`ModelBuilder`] by name, so this test genuinely
/// references the type rather than only ever inferring it.
fn configure(builder: ModelBuilder) -> ModelBuilder {
    builder
        .retry_policy(RetryPolicy::default())
        .default_max_tokens(256)
        .default_response_format(ResponseFormat {
            name: "answer".to_owned(),
            schema: serde_json::json!({"type": "object"}),
        })
}

#[tokio::test]
async fn model_calls_are_reachable_through_the_facade() {
    let model = configure(Model::builder(Arc::new(StubProvider), "stub-model")).build();
    let request = CompletionRequest::new(vec![Message::user("hi")]);

    let completion = model
        .complete(request.clone())
        .await
        .expect("the stub provider must succeed");
    let expected = Completion {
        content: vec![Content::Text("hello".to_owned())],
        usage: Usage {
            input_tokens: Some(1),
            output_tokens: Some(1),
        },
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "stub-model".to_owned(),
    };
    assert_eq!(
        completion, expected,
        "the facade must re-export Completion, StopReason and Usage together"
    );

    let mut stream = model.stream(request).await.expect("stream must open");
    let mut accumulator = CompletionAccumulator::new();
    while let Some(event) = stream.next().await {
        accumulator.push(event.expect("the stub sequence must be well-formed"));
    }
    let replayed = accumulator
        .finish()
        .expect("a well-formed sequence must fold");
    assert_eq!(
        replayed, completion,
        "the facade's CompletionAccumulator must reassemble what Model::complete returns"
    );
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn scripted_provider_is_reachable_through_the_facade() {
    use grizzly_agent::{ScriptedProvider, ScriptedResponse};

    let provider = ScriptedProvider::new(vec![ScriptedResponse::Completion(Completion {
        content: vec![Content::Text("scripted".to_owned())],
        usage: Usage::default(),
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "scripted-model".to_owned(),
    })]);
    let model = Model::builder(Arc::new(provider), "scripted-model").build();

    let completion = model
        .complete(CompletionRequest::new(vec![Message::user("hi")]))
        .await
        .expect("the scripted provider must succeed");

    assert_eq!(
        completion.content,
        vec![Content::Text("scripted".to_owned())],
        "the facade must re-export ScriptedProvider and ScriptedResponse"
    );
}
