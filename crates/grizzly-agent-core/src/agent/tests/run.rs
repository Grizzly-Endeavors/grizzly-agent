//! Tests for [`super`].

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};

use super::*;
use crate::agent::section::DynamicSection;
use crate::completion::CompletionEvent;
use crate::retry::RetryPolicy;
use crate::test_support::{ScriptedProvider, ScriptedResponse};
use crate::tools::{ToolHandler, ToolSpec};

fn text_completion(text: &str) -> Completion {
    Completion {
        content: vec![Content::Text(text.to_owned())],
        usage: Usage {
            input_tokens: Some(5),
            output_tokens: Some(1),
        },
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "test-model".to_owned(),
    }
}

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

fn max_tokens_completion(text: &str) -> Completion {
    Completion {
        content: vec![
            Content::Text(text.to_owned()),
            Content::ToolUse(ToolUse {
                id: "cut-off".to_owned(),
                name: "echo".to_owned(),
                input: serde_json::json!({}),
            }),
        ],
        usage: Usage {
            input_tokens: Some(20),
            output_tokens: Some(3),
        },
        stop_reason: StopReason::MaxTokens,
        raw_stop_reason: "max_tokens".to_owned(),
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

fn tool_set(handlers: Vec<Box<dyn ToolHandler>>) -> ToolSet {
    ToolSet::new(handlers).expect("test tool sets never collide on names")
}

fn system_text(request: &CompletionRequest) -> &str {
    match request.messages.first() {
        Some(Message {
            role: Role::System,
            content,
        }) => match content.as_slice() {
            [Content::Text(text)] => text,
            _ => panic!("the system message must be a single text block"),
        },
        _ => panic!("the request must open with a system message"),
    }
}

struct EchoTool;

#[async_trait::async_trait]
impl ToolHandler for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("echo"),
            description: Cow::Borrowed("echoes its arguments"),
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

struct StopTool;

#[async_trait::async_trait]
impl ToolHandler for StopTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("stop"),
            description: Cow::Borrowed("hands the run to a human"),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn call(
        &self,
        _arguments: serde_json::Value,
        context: &ToolContext,
    ) -> Result<String, ToolFailure> {
        assert!(
            context.request_stop("handing off", "needs a human"),
            "this test's tool context is fresh, so nothing could have requested a stop first"
        );
        Ok("stopping".to_owned())
    }
}

struct CancelTool;

#[async_trait::async_trait]
impl ToolHandler for CancelTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("cancel"),
            description: Cow::Borrowed("cancels the run"),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn call(
        &self,
        _arguments: serde_json::Value,
        context: &ToolContext,
    ) -> Result<String, ToolFailure> {
        context.cancellation_token().cancel();
        Ok("cancelling".to_owned())
    }
}

struct Toggle(Arc<Mutex<String>>);

impl DynamicSection for Toggle {
    fn render(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct SetSectionTool(Arc<Mutex<String>>);

#[async_trait::async_trait]
impl ToolHandler for SetSectionTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("set_section"),
            description: Cow::Borrowed("changes shared section state"),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    async fn call(
        &self,
        _arguments: serde_json::Value,
        _context: &ToolContext,
    ) -> Result<String, ToolFailure> {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = "changed".to_owned();
        Ok("ok".to_owned())
    }
}

#[derive(Default, Clone)]
struct RecordingObserver {
    events: Arc<Mutex<Vec<CompletionEvent>>>,
    rounds: Arc<Mutex<Vec<RoundRecord>>>,
    tool_calls: Arc<Mutex<Vec<ToolCallRecord>>>,
}

#[async_trait::async_trait]
impl RunObserver for RecordingObserver {
    async fn on_event(&self, event: &CompletionEvent) {
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event.clone());
    }

    async fn on_round(&self, round: &RoundRecord) {
        self.rounds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(round.clone());
    }

    async fn on_tool_call(&self, call: &ToolCallRecord) {
        self.tool_calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call.clone());
    }
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn agent_is_send_and_sync() {
    assert_send_sync::<Agent>();
}

#[tokio::test]
async fn a_system_message_in_the_conversation_is_rejected_upfront() {
    let (model, provider) = scripted(vec![ScriptedResponse::Completion(text_completion(
        "unreachable",
    ))]);
    let agent = Agent::builder(model, tool_set(vec![])).build();

    let error = agent
        .run(
            vec![Message::system("be nice"), Message::user("hi")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect_err("a system message must be rejected before any work begins");

    assert!(matches!(error, RunFailure::InvalidConversation { .. }));
    assert_eq!(
        provider.calls_served(),
        0,
        "the model must never be called once the upfront check fails"
    );
}

#[tokio::test]
async fn a_broken_request_invariant_is_also_rejected_upfront() {
    let (model, _provider) = scripted(Vec::new());
    let agent = Agent::builder(model, tool_set(vec![])).build();

    let dangling_result = Message::tool_results(vec![ToolResult {
        tool_use_id: "never-asked-for".to_owned(),
        content: "ok".to_owned(),
        is_error: false,
    }]);

    let error = agent
        .run(vec![dangling_result], CancellationToken::new(), None)
        .await
        .expect_err("a tool result answering no known tool use must be rejected");

    assert!(
        matches!(error, RunFailure::InvalidConversation { reason } if reason.contains("tool result")),
        "the reason must name the broken invariant"
    );
}

#[tokio::test]
async fn completed_ending_carries_the_final_reply() {
    let (model, _provider) = scripted(vec![ScriptedResponse::Completion(text_completion(
        "all done",
    ))]);
    let agent = Agent::builder(model, tool_set(vec![])).build();

    let record = agent
        .run(vec![Message::user("hi")], CancellationToken::new(), None)
        .await
        .expect("a plain reply must complete the run");

    assert_eq!(record.ending, RunEnding::Completed);
    assert_eq!(record.reply, Some("all done".to_owned()));
    assert_eq!(record.trace.rounds.len(), 1);
    assert!(
        record
            .trace
            .messages
            .iter()
            .any(|message| message.role == Role::Assistant)
    );
}

#[tokio::test]
async fn truncated_ending_strips_tool_uses_and_a_resumed_conversation_is_accepted() {
    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(max_tokens_completion("partial thought")),
        ScriptedResponse::Completion(text_completion("finished")),
    ]);
    let agent = Agent::builder(model, tool_set(vec![Box::new(EchoTool)])).build();
    let original = vec![Message::user("hi")];

    let first = agent
        .run(original.clone(), CancellationToken::new(), None)
        .await
        .expect("hitting the token limit is not a run failure");

    assert_eq!(first.ending, RunEnding::Truncated);
    assert_eq!(first.reply, Some("partial thought".to_owned()));
    let assistant_message = first
        .trace
        .messages
        .iter()
        .find(|message| message.role == Role::Assistant)
        .expect("the truncated round still appends its assistant message");
    assert!(
        !assistant_message
            .content
            .iter()
            .any(|block| matches!(block, Content::ToolUse(_))),
        "tool-use blocks must be stripped from the appended message"
    );

    let mut resumed = original;
    resumed.extend(first.trace.messages);
    let second = agent
        .run(resumed, CancellationToken::new(), None)
        .await
        .expect("a resumed, tool-use-free transcript must satisfy the request invariants");
    assert_eq!(second.ending, RunEnding::Completed);
}

#[tokio::test]
async fn stop_requested_ending_carries_the_tool_s_reply() {
    let (model, _provider) = scripted(vec![ScriptedResponse::Completion(tool_use_completion(
        "call-1",
        "stop",
        serde_json::json!({}),
    ))]);
    let agent = Agent::builder(model, tool_set(vec![Box::new(StopTool)])).build();

    let record = agent
        .run(
            vec![Message::user("please stop")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect("a stop request is a normal ending, not a failure");

    assert!(matches!(
        &record.ending,
        RunEnding::StopRequested(stop) if stop.reply == "handing off" && stop.reason == "needs a human"
    ));
    assert_eq!(record.reply, Some("handing off".to_owned()));
}

#[tokio::test]
async fn rounds_exhausted_after_the_round_limit() {
    let responses = (0..3)
        .map(|i| {
            ScriptedResponse::Completion(tool_use_completion(
                &format!("call-{i}"),
                "echo",
                serde_json::json!({"i": i}),
            ))
        })
        .collect();
    let (model, provider) = scripted(responses);
    let agent = Agent::builder(model, tool_set(vec![Box::new(EchoTool)]))
        .limits(Limits {
            round_limit: 2,
            stall_threshold: 100,
        })
        .build();

    let record = agent
        .run(
            vec![Message::user("keep going")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect("running out of rounds is a normal ending, not a failure");

    assert_eq!(record.ending, RunEnding::RoundsExhausted);
    assert_eq!(record.trace.rounds.len(), 2);
    assert_eq!(
        provider.calls_served(),
        2,
        "the loop must not call the model a third time"
    );
}

#[tokio::test]
async fn stalled_ending_after_repeating_the_same_call() {
    let identical = serde_json::json!({"target": "same"});
    let responses = (0..5)
        .map(|i| {
            ScriptedResponse::Completion(tool_use_completion(
                &format!("call-{i}"),
                "echo",
                identical.clone(),
            ))
        })
        .collect();
    let (model, _provider) = scripted(responses);
    let agent = Agent::builder(model, tool_set(vec![Box::new(EchoTool)]))
        .limits(Limits {
            round_limit: 16,
            stall_threshold: 4,
        })
        .build();

    let record = agent
        .run(
            vec![Message::user("loop please")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect("stalling is a normal ending, not a failure");

    assert_eq!(
        record.ending,
        RunEnding::Stalled {
            tool: "echo".to_owned(),
            count: 4
        }
    );
    assert_eq!(
        record.trace.rounds.len(),
        4,
        "the loop must stop right after the fourth identical call"
    );
}

#[tokio::test]
async fn cancelled_ending_when_the_token_is_already_set() {
    let (model, provider) = scripted(vec![ScriptedResponse::Completion(text_completion(
        "unreachable",
    ))]);
    let agent = Agent::builder(model, tool_set(vec![])).build();
    let token = CancellationToken::new();
    token.cancel();

    let record = agent
        .run(vec![Message::user("hi")], token, None)
        .await
        .expect("a cancellation before work begins is a normal ending, not a failure");

    assert_eq!(record.ending, RunEnding::Cancelled);
    assert!(record.trace.rounds.is_empty());
    assert_eq!(
        provider.calls_served(),
        0,
        "a cancelled run must not start further work"
    );
}

#[tokio::test]
async fn cancelled_ending_mid_tool_dispatch() {
    let completion = Completion {
        content: vec![
            Content::ToolUse(ToolUse {
                id: "call-cancel".to_owned(),
                name: "cancel".to_owned(),
                input: serde_json::json!({}),
            }),
            Content::ToolUse(ToolUse {
                id: "call-echo".to_owned(),
                name: "echo".to_owned(),
                input: serde_json::json!({}),
            }),
        ],
        usage: Usage {
            input_tokens: Some(10),
            output_tokens: Some(2),
        },
        stop_reason: StopReason::ToolUse,
        raw_stop_reason: "tool_use".to_owned(),
        model: "test-model".to_owned(),
    };
    let (model, _provider) = scripted(vec![ScriptedResponse::Completion(completion)]);
    let agent = Agent::builder(
        model,
        tool_set(vec![Box::new(CancelTool), Box::new(EchoTool)]),
    )
    .build();

    let record = agent
        .run(
            vec![Message::user("cancel me")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect("a mid-run cancellation is a normal ending, not a failure");

    assert_eq!(record.ending, RunEnding::Cancelled);
    assert_eq!(record.trace.rounds.len(), 1);
    let round = record
        .trace
        .rounds
        .first()
        .expect("the round that was interrupted must still be recorded");
    assert_eq!(
        round.tool_calls.len(),
        1,
        "cancellation must be checked before the second tool dispatch, so echo never runs"
    );
}

#[tokio::test]
async fn a_failing_tool_becomes_a_result_and_the_run_continues() {
    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(tool_use_completion("call-1", "fail", serde_json::json!({}))),
        ScriptedResponse::Completion(text_completion("recovered")),
    ]);
    let agent = Agent::builder(model, tool_set(vec![Box::new(FailingTool)])).build();

    let record = agent
        .run(
            vec![Message::user("try it")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect("a failing tool must not end the run");

    assert_eq!(record.ending, RunEnding::Completed);
    assert_eq!(record.reply, Some("recovered".to_owned()));
    let call = record
        .trace
        .rounds
        .first()
        .and_then(|round| round.tool_calls.first())
        .expect("the failing call must still be recorded");
    assert!(call.failed);

    let results_message = record
        .trace
        .messages
        .iter()
        .find(|message| message.role == Role::User)
        .expect("the round's tool results must be appended");
    assert!(matches!(
        results_message.content.as_slice(),
        [Content::ToolResult(result)] if result.is_error
    ));
}

#[tokio::test]
async fn a_later_provider_failure_returns_err_with_the_earlier_trace() {
    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(tool_use_completion("call-1", "echo", serde_json::json!({}))),
        ScriptedResponse::PreStreamFailure(ProviderFailure::Status {
            provider: "test".to_owned(),
            status: 400,
            message: "boom".to_owned(),
            retry_after: None,
        }),
    ]);
    let agent = Agent::builder(model, tool_set(vec![Box::new(EchoTool)])).build();

    let error = agent
        .run(vec![Message::user("hi")], CancellationToken::new(), None)
        .await
        .expect_err("a provider failure must surface as an error");

    let RunFailure::Provider { source, trace } = &error else {
        panic!("expected RunFailure::Provider, got {error:?}");
    };
    assert!(matches!(
        source,
        ProviderFailure::Status { status: 400, .. }
    ));
    assert_eq!(
        trace.rounds.len(),
        1,
        "the earlier successful round must survive in the trace"
    );

    let boxed_source = std::error::Error::source(&error)
        .expect("the provider failure must be attached as the source");
    assert!(boxed_source.to_string().contains("boom"));
}

#[tokio::test]
async fn a_provider_failure_propagates_through_question_mark_into_anyhow() {
    async fn run_agent(agent: &Agent) -> anyhow::Result<RunRecord> {
        let record = agent
            .run(vec![Message::user("hi")], CancellationToken::new(), None)
            .await?;
        Ok(record)
    }

    let (model, _provider) = scripted(vec![ScriptedResponse::PreStreamFailure(
        ProviderFailure::Status {
            provider: "test".to_owned(),
            status: 400,
            message: "boom".to_owned(),
            retry_after: None,
        },
    )]);
    let agent = Agent::builder(model, tool_set(vec![])).build();

    let error = run_agent(&agent)
        .await
        .expect_err("the provider failure must propagate through `?`");
    let chain: Vec<String> = error
        .chain()
        .map(std::string::ToString::to_string)
        .collect();
    assert!(
        chain.iter().any(|frame| frame.contains("boom")),
        "the anyhow chain must name the provider failure, got: {chain:?}"
    );
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

#[tokio::test]
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
    let agent = Agent::builder(model, tool_set(vec![Box::new(FailingTool)])).build();

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

#[tokio::test]
async fn two_concurrent_runs_only_see_their_own_observer_events() {
    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(text_completion("hi")),
        ScriptedResponse::Completion(text_completion("hi")),
    ]);
    let agent = Agent::builder(model, tool_set(vec![])).build();

    let observer_a = RecordingObserver::default();
    let observer_b = RecordingObserver::default();
    let handle_a = observer_a.clone();
    let handle_b = observer_b.clone();

    let (result_a, result_b) = tokio::join!(
        agent.run(
            vec![Message::user("a")],
            CancellationToken::new(),
            Some(Box::new(observer_a) as Box<dyn RunObserver>)
        ),
        agent.run(
            vec![Message::user("b")],
            CancellationToken::new(),
            Some(Box::new(observer_b) as Box<dyn RunObserver>)
        ),
    );

    result_a.expect("run a must complete");
    result_b.expect("run b must complete");

    let events_a = handle_a
        .events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len();
    let events_b = handle_b
        .events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len();
    assert!(
        events_a > 0 && events_b > 0,
        "each observer must see its own run's events"
    );
    assert_eq!(
        events_a, events_b,
        "both runs are identical single-round completions, so each observer must see exactly \
         one run's worth of events, not both runs' events merged"
    );
}

#[tokio::test]
async fn the_observer_sees_every_event_round_and_tool_call_in_order() {
    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(tool_use_completion(
            "call-1",
            "echo",
            serde_json::json!({"n": 1}),
        )),
        ScriptedResponse::Completion(text_completion("done")),
    ]);
    let agent = Agent::builder(model, tool_set(vec![Box::new(EchoTool)])).build();
    let observer = RecordingObserver::default();
    let handle = observer.clone();

    let record = agent
        .run(
            vec![Message::user("go")],
            CancellationToken::new(),
            Some(Box::new(observer)),
        )
        .await
        .expect("a normal two-round run must complete");

    assert_eq!(record.trace.rounds.len(), 2);

    let rounds = handle.rounds.lock().unwrap_or_else(PoisonError::into_inner);
    assert_eq!(
        *rounds, record.trace.rounds,
        "the observer must see every round, in order"
    );
    drop(rounds);

    let tool_calls = handle
        .tool_calls
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(
        tool_calls.first().map(|call| call.name.as_str()),
        Some("echo")
    );
    drop(tool_calls);

    let events = handle.events.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(
        events.iter().any(
            |event| matches!(event, CompletionEvent::ToolUseStart { name, .. } if name == "echo")
        ),
        "the observer must see the streamed tool-use-start event"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, CompletionEvent::TextDelta(text) if text == "done")),
        "the observer must see the streamed text from the final round"
    );
}

#[tokio::test]
async fn sections_drop_empty_entries_join_with_a_blank_line_and_reflect_mid_run_changes() {
    let shared = Arc::new(Mutex::new("initial".to_owned()));
    let (model, provider) = scripted(vec![
        ScriptedResponse::Completion(tool_use_completion(
            "call-1",
            "set_section",
            serde_json::json!({}),
        )),
        ScriptedResponse::Completion(text_completion("done")),
    ]);
    let agent = Agent::builder(
        model,
        tool_set(vec![Box::new(SetSectionTool(Arc::clone(&shared)))]),
    )
    .section("preamble")
    .section("")
    .section(SystemSection::dynamic(Toggle(Arc::clone(&shared))))
    .build();

    agent
        .run(vec![Message::user("go")], CancellationToken::new(), None)
        .await
        .expect("a two-round run must complete");

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);

    let first_request = requests.first().expect("the first round's request");
    let second_request = requests.get(1).expect("the second round's request");
    assert_eq!(
        system_text(first_request),
        "preamble\n\ninitial",
        "the empty section must be dropped and the rest joined with a blank line"
    );
    assert_eq!(
        system_text(second_request),
        "preamble\n\nchanged",
        "the dynamic section must render fresh, reflecting the tool's change, on the next round"
    );
}

#[tokio::test]
async fn total_usage_is_unknown_when_any_round_s_is() {
    let known = tool_use_completion("call-1", "echo", serde_json::json!({}));
    let mut unknown_output = text_completion("done");
    unknown_output.usage.output_tokens = None;

    let (model, _provider) = scripted(vec![
        ScriptedResponse::Completion(known),
        ScriptedResponse::Completion(unknown_output),
    ]);
    let agent = Agent::builder(model, tool_set(vec![Box::new(EchoTool)])).build();

    let record = agent
        .run(vec![Message::user("hi")], CancellationToken::new(), None)
        .await
        .expect("a normal two-round run must complete");

    assert_eq!(
        record.trace.total_usage.input_tokens,
        Some(15),
        "both rounds reported input tokens"
    );
    assert_eq!(
        record.trace.total_usage.output_tokens, None,
        "the second round's output tokens were unknown"
    );
}

#[test]
fn stall_tracker_resets_on_a_different_call_and_ignores_key_order() {
    let mut stall = StallTracker::default();

    stall.record("echo", &serde_json::json!({"a": 1, "b": 2}));
    assert_eq!(stall.streak, 1);

    stall.record("echo", &serde_json::json!({"b": 2, "a": 1}));
    assert_eq!(
        stall.streak, 2,
        "key-reordered arguments must compare as identical"
    );

    stall.record("other", &serde_json::json!({}));
    assert_eq!(
        stall.streak, 1,
        "a different call must reset the streak to one"
    );

    stall.record("echo", &serde_json::json!({"a": 1, "b": 2}));
    assert_eq!(
        stall.streak, 1,
        "the call right before this one was different, so this one also resets to one"
    );
}

#[test]
fn stall_tracker_trips_at_the_threshold() {
    let mut stall = StallTracker::default();
    for _ in 0..3 {
        stall.record("echo", &serde_json::json!({}));
    }
    assert_eq!(
        stall.tripped(4),
        None,
        "three repeats must not yet trip a threshold of four"
    );

    stall.record("echo", &serde_json::json!({}));
    assert_eq!(stall.tripped(4), Some(("echo".to_owned(), 4)));
}
