//! Every item the facade re-exports, used through `grizzly_agent::` the way
//! a consumer would, so a missing re-export is a compile failure here rather
//! than a surprise in a downstream crate.
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use std::sync::Arc;

use futures_util::StreamExt;
use std::borrow::Cow;
use std::sync::Mutex;

use grizzly_agent::{
    Agent, AgentBuilder, Completion, CompletionAccumulator, CompletionEvent, CompletionRequest,
    CompletionStream, Content, DuplicateToolName, DynamicSection, Limits, Message, Model,
    ModelBuilder, NoParams, Provider, ProviderFailure, ResponseFormat, RetryPolicy, Role,
    RoundRecord, RunEnding, RunFailure, RunObserver, RunRecord, RunTrace, StopReason, StopRequest,
    SystemSection, ToolCallRecord, ToolContext, ToolDefinition, ToolFailure, ToolHandler,
    ToolResult, ToolSet, ToolSpec, ToolUse, TypedToolHandler, Usage,
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
    let tool_failure = ToolFailure::Unknown {
        name: "missing".to_owned(),
        available: Vec::new(),
    };
    let run_failure = RunFailure::InvalidConversation {
        reason: "system messages must appear only at the head".to_owned(),
    };

    assert!(
        matches!(run_failure, RunFailure::InvalidConversation { .. }),
        "the facade must re-export RunFailure"
    );
    assert!(
        tool_failure.into_result("call-1").is_error,
        "the facade must re-export ToolFailure"
    );

    let provider_failure = ProviderFailure::InvalidRequest("bad request".to_owned());
    let wrapped = RunFailure::Provider {
        source: provider_failure,
        trace: Box::new(RunTrace {
            messages: Vec::new(),
            rounds: Vec::new(),
            total_usage: Usage::default(),
        }),
    };
    assert!(
        matches!(wrapped, RunFailure::Provider { .. }),
        "the facade must re-export RunFailure and ProviderFailure together"
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

#[cfg(feature = "eval")]
#[test]
fn eval_shared_core_is_reachable_through_the_facade() {
    use grizzly_agent::{
        CaseAggregate, CaseMeta, CaseReport, CheckResult, DEFAULT_MIN_PASS_RATE, DEFAULT_REPEATS,
        InvocationDir, InvocationDirError, REPORT_FILE, REPORT_SCHEMA_VERSION, RepeatRecord,
        Report, Verdict, VerdictCategory, render_summary,
    };

    let meta = CaseMeta {
        min_pass_rate: Some(DEFAULT_MIN_PASS_RATE),
        repeats: Some(DEFAULT_REPEATS),
        ..CaseMeta::new("facade-case")
    };
    let verdict = Verdict::pass("answered correctly");
    assert_eq!(
        verdict.category,
        VerdictCategory::Pass,
        "the facade must re-export VerdictCategory alongside Verdict"
    );

    let aggregate = CaseAggregate::aggregate(&meta, std::slice::from_ref(&verdict));
    let repeats = vec![RepeatRecord::new(verdict)];
    let case_report = CaseReport { aggregate, repeats };
    let report = Report::new(
        uuid::Uuid::now_v7(),
        jiff::Timestamp::now(),
        serde_json::json!({}),
        vec![case_report],
    );
    assert_eq!(
        report.schema_version, REPORT_SCHEMA_VERSION,
        "the facade must re-export REPORT_SCHEMA_VERSION matching Report::new's stamp"
    );
    assert!(
        report.suite_result().all_met,
        "the facade must re-export a Report whose suite_result reflects a passing case"
    );
    assert!(
        !render_summary(&report).is_empty(),
        "the facade must re-export a working render_summary"
    );

    let check = CheckResult::passed("lint", "exit 0");
    assert!(check.passed, "the facade must re-export CheckResult");

    let root = std::env::temp_dir().join(format!(
        "grizzly-agent-facade-test-{}",
        uuid::Uuid::now_v7()
    ));
    let dir =
        InvocationDir::create(&root).expect("the facade's InvocationDir must create its directory");
    assert_eq!(
        dir.report_path()
            .file_name()
            .and_then(std::ffi::OsStr::to_str),
        Some(REPORT_FILE),
        "the facade must re-export REPORT_FILE matching InvocationDir::report_path"
    );
    let written: Result<_, InvocationDirError> = dir.write_report(&report);
    assert!(
        written.is_ok(),
        "the facade's InvocationDir must write a report successfully"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[cfg(all(feature = "eval", feature = "test-support"))]
#[tokio::test]
async fn response_eval_runner_is_reachable_through_the_facade() {
    use grizzly_agent::{CaseMeta, CaseTimeout, ResponseEvalCase, ResponseEvalRunner, Verdict};

    use grizzly_agent::{ScriptedProvider, ScriptedResponse};

    struct AlwaysRight {
        meta: CaseMeta,
    }

    impl ResponseEvalCase for AlwaysRight {
        type Answer = String;

        fn meta(&self) -> &CaseMeta {
            &self.meta
        }

        fn build_request(&self) -> CompletionRequest {
            CompletionRequest::new(vec![Message::user("hi")])
        }

        fn parse(&self, completion: &Completion) -> Result<String, String> {
            Ok(completion
                .content
                .first()
                .map_or_else(String::new, |block| match block {
                    Content::Text(text) => text.clone(),
                    Content::Reasoning { .. } | Content::ToolUse(_) | Content::ToolResult(_) => {
                        String::new()
                    }
                }))
        }

        fn score(&self, _answer: &String) -> Verdict {
            Verdict::pass("always right")
        }

        fn timeout(&self) -> CaseTimeout {
            CaseTimeout::None
        }
    }

    let provider = ScriptedProvider::new(vec![ScriptedResponse::Completion(Completion {
        content: vec![Content::Text("scripted".to_owned())],
        usage: Usage::default(),
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "scripted-model".to_owned(),
    })]);
    let model = Model::builder(Arc::new(provider), "scripted-model").build();
    let runner = ResponseEvalRunner::builder(model, 1).build();
    let case = AlwaysRight {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("facade-response-eval")
        },
    };

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let report = reports
        .first()
        .expect("the facade's ResponseEvalRunner must produce one report per case");
    assert_eq!(
        report.aggregate.passes, 1,
        "the facade's ResponseEvalRunner must run the case and record its pass"
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

#[test]
fn the_hidden_serde_json_re_export_is_reachable_through_the_facade() {
    let value = grizzly_agent::serde_json::json!({"ok": true});
    assert_eq!(
        value,
        serde_json::json!({"ok": true}),
        "the facade must re-export the same serde_json codegen depends on"
    );
}

struct StaticMood;

impl DynamicSection for StaticMood {
    fn render(&self) -> String {
        "current mood: chill".to_owned()
    }
}

struct RecordingObserver {
    events: Mutex<Vec<CompletionEvent>>,
    rounds: Mutex<Vec<RoundRecord>>,
    tool_calls: Mutex<Vec<ToolCallRecord>>,
}

#[async_trait::async_trait]
impl RunObserver for RecordingObserver {
    async fn on_event(&self, event: &CompletionEvent) {
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(event.clone());
    }

    async fn on_round(&self, round: &RoundRecord) {
        self.rounds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(round.clone());
    }

    async fn on_tool_call(&self, call: &ToolCallRecord) {
        self.tool_calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(call.clone());
    }
}

/// Takes and returns an [`AgentBuilder`] by name, so this test genuinely
/// references the type rather than only ever inferring it.
fn configure_agent(builder: AgentBuilder) -> AgentBuilder {
    builder
        .section("static preamble")
        .section(SystemSection::dynamic(StaticMood))
        .limits(Limits {
            round_limit: 4,
            stall_threshold: 4,
        })
}

#[cfg(feature = "test-support")]
#[tokio::test]
async fn the_turn_loop_and_agent_are_reachable_through_the_facade() {
    use grizzly_agent::{ScriptedProvider, ScriptedResponse};

    let tool_round = Completion {
        content: vec![Content::ToolUse(ToolUse {
            id: "call-1".to_owned(),
            name: "echo".to_owned(),
            input: serde_json::json!({}),
        })],
        usage: Usage {
            input_tokens: Some(10),
            output_tokens: Some(2),
        },
        stop_reason: StopReason::ToolUse,
        raw_stop_reason: "tool_use".to_owned(),
        model: "scripted-model".to_owned(),
    };
    let final_round = Completion {
        content: vec![Content::Text("hi there".to_owned())],
        usage: Usage {
            input_tokens: Some(5),
            output_tokens: Some(1),
        },
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "scripted-model".to_owned(),
    };
    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::Completion(tool_round),
        ScriptedResponse::Completion(final_round),
    ]);
    let model = Model::builder(Arc::new(provider), "scripted-model").build();

    let tools = ToolSet::new([Box::new(EchoTool) as Box<dyn ToolHandler>])
        .expect("a single tool registers cleanly");

    let agent = configure_agent(Agent::builder(model, tools)).build();

    let observer = RecordingObserver {
        events: Mutex::new(Vec::new()),
        rounds: Mutex::new(Vec::new()),
        tool_calls: Mutex::new(Vec::new()),
    };

    let record: RunRecord = agent
        .run(
            vec![Message::user("hello")],
            tokio_util::sync::CancellationToken::new(),
            Some(Box::new(observer)),
        )
        .await
        .expect("a scripted, tool-using run must complete through the facade");

    assert_eq!(
        record.ending,
        RunEnding::Completed,
        "the facade must re-export RunEnding and a working turn loop"
    );
    assert_eq!(record.reply.as_deref(), Some("hi there"));
    assert_eq!(
        record.trace.rounds.len(),
        2,
        "the facade must re-export RunRecord and RunTrace with populated round records"
    );
}
