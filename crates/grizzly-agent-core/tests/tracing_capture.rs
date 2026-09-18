//! Asserts on the actual `tracing` events [`Agent::run`] emits.
//!
//! `tracing_core`'s callsite interest cache is process-wide: the first time
//! any thread in a process hits a given `tracing::warn!`/`tracing::error!`
//! call site, the result of that check is cached for the rest of the
//! process, keyed by call site rather than by thread or subscriber. A unit
//! test elsewhere in this crate that drives the same call sites (any other
//! test exercising a failing tool or a provider failure) without installing
//! a subscriber races that cache against this test's `set_default` when both
//! run concurrently in the same test binary, which can silently drop this
//! test's own events. Living in its own integration-test binary gives this
//! test its own process and its own callsite cache, closing that race
//! entirely rather than narrowing it.
#![cfg(feature = "test-support")]
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};

use grizzly_agent_core::{
    Agent, Completion, Content, Message, Model, ProviderFailure, RetryPolicy, ScriptedProvider,
    ScriptedResponse, StopReason, ToolContext, ToolFailure, ToolHandler, ToolSet, ToolSpec,
    ToolUse, Usage,
};
use tokio_util::sync::CancellationToken;

fn tool_use_completion(id: &str, name: &str, input: serde_json::Value) -> Completion {
    Completion {
        content: vec![Content::ToolUse(ToolUse {
            id: id.to_owned(),
            name: name.to_owned(),
            input,
        })],
        usage: Usage {
            input_tokens: Some(10),
            output_tokens: Some(2),
        },
        stop_reason: StopReason::ToolUse,
        raw_stop_reason: "tool_use".to_owned(),
        model: "test-model".to_owned(),
    }
}

fn scripted(responses: Vec<ScriptedResponse>) -> (Model, ScriptedProvider) {
    let provider = ScriptedProvider::new(responses);
    let model = Model::builder(Arc::new(provider.clone()), "test-model")
        .retry_policy(RetryPolicy::none())
        .build();
    (model, provider)
}

struct FailingTool;

#[async_trait::async_trait]
impl ToolHandler for FailingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("fail"),
            description: Cow::Borrowed("always fails"),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn call(
        &self,
        _arguments: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<String, ToolFailure> {
        Err(ToolFailure::Execution {
            name: "fail".to_owned(),
            message: "boom".to_owned(),
        })
    }
}

#[derive(Clone)]
struct BufferWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for BufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufferWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn tool_failure_and_run_failure_are_logged() {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(BufferWriter(Arc::clone(&buffer)))
        .with_ansi(false)
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);

    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(tool_use_completion("call-1", "fail", serde_json::json!({}))),
        ScriptedResponse::PreStreamFailure(ProviderFailure::Status {
            provider: "test".to_owned(),
            status: 400,
            message: "boom".to_owned(),
            retry_after: None,
        }),
    ]);
    let handlers: Vec<Box<dyn ToolHandler>> = vec![Box::new(FailingTool)];
    let agent = Agent::builder(
        model,
        ToolSet::new(handlers).expect("test tool sets never collide on names"),
    )
    .build();

    let outcome = agent
        .run(vec![Message::user("hi")], CancellationToken::new(), None)
        .await;
    assert!(
        outcome.is_err(),
        "the second round's provider failure must surface"
    );

    drop(guard);
    let output = String::from_utf8(
        buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    )
    .expect("tracing output must be valid utf-8");

    assert!(
        output.contains("WARN") && output.contains("tool call failed"),
        "the tool failure must be logged as a warning, got: {output}"
    );
    assert!(
        output.contains("ERROR") && output.contains("run failed"),
        "the run failure must be logged as an error, got: {output}"
    );
}
