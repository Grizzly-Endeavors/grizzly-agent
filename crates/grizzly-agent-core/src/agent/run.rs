//! [`Agent`]: a configured, reusable harness combining a [`Model`], a
//! [`ToolSet`], a system prompt built from sections, and limits — and the
//! turn loop that drives a conversation through it.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::accumulator::CompletionAccumulator;
use crate::agent::observer::RunObserver;
use crate::agent::section::{SystemSection, render_system_prompt};
use crate::agent::trace::{RoundRecord, RunEnding, RunRecord, RunTrace, ToolCallRecord};
use crate::completion::{Completion, StopReason, Usage};
use crate::error::{ProviderFailure, RunFailure, ToolFailure};
use crate::message::{Content, Message, Role, ToolResult, ToolUse};
use crate::model::Model;
use crate::request::{CompletionRequest, validate_messages};
use crate::tools::{ToolContext, ToolSet};

/// Backstops on a run's length: a last-resort round limit, and a stall
/// threshold for a tool call repeating without progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The maximum number of model rounds a run will make before ending as
    /// [`RunEnding::RoundsExhausted`].
    pub round_limit: u32,
    /// How many consecutive, structurally identical dispatched tool calls
    /// end a run as [`RunEnding::Stalled`].
    pub stall_threshold: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            round_limit: 16,
            stall_threshold: 4,
        }
    }
}

/// A configured, reusable harness: a [`Model`], a [`ToolSet`], a system
/// prompt, and limits. There is no agent trait — harnesses are built on this
/// loop.
///
/// `Agent` is `Send + Sync`, and [`Agent::run`] takes `&self`, so one
/// `Agent` may serve many concurrent runs, one per conversation; nothing in
/// the `Agent` is mutated by a run. Per-run state belongs to the run.
pub struct Agent {
    model: Model,
    tools: ToolSet,
    sections: Vec<SystemSection>,
    limits: Limits,
}

impl Agent {
    /// Starts building an `Agent` around `model` and `tools`.
    #[must_use]
    pub fn builder(model: Model, tools: ToolSet) -> AgentBuilder {
        AgentBuilder {
            model,
            tools,
            sections: Vec::new(),
            limits: Limits::default(),
        }
    }

    /// Runs `conversation` — the caller's history plus the new input — to a
    /// defined stopping point.
    ///
    /// Before any work begins, `conversation` must not contain a
    /// system-role message (this `Agent`'s sections are the only system
    /// prompt) and must satisfy the request invariants [`Model`] enforces.
    /// From there the loop calls the model, dispatches any requested tools,
    /// and repeats until it reaches one of [`RunEnding`]'s stopping points.
    ///
    /// `Err` means the system broke; `Ok` means the loop ran to a defined
    /// stopping point, whatever the agent decided.
    ///
    /// # Errors
    /// Returns [`RunFailure::InvalidConversation`] naming the broken rule if
    /// `conversation` fails the upfront check, or [`RunFailure::Provider`]
    /// if a model call fails after retries are exhausted — carrying every
    /// round the run completed before the failure. Either way this logs a
    /// `tracing` error event, so a failure reaches logs even if the caller
    /// mishandles the `Err`.
    pub async fn run(
        &self,
        conversation: Vec<Message>,
        cancellation_token: CancellationToken,
        observer: Option<Box<dyn RunObserver>>,
    ) -> Result<RunRecord, RunFailure> {
        if let Err(failure) = validate_conversation(&conversation) {
            log_run_failure(&failure);
            return Err(failure);
        }

        let observer = observer.as_deref();
        let mut tool_context = ToolContext::new(cancellation_token.clone());
        let mut state = RunState::default();
        let mut stall = StallTracker::default();

        for _round in 1..=self.limits.round_limit {
            if cancellation_token.is_cancelled() {
                return Ok(state.into_record(RunEnding::Cancelled, None));
            }

            let inputs = RoundInputs {
                tool_context: &tool_context,
                cancellation_token: &cancellation_token,
                observer,
            };

            match self
                .run_round(&conversation, &mut state, &inputs, &mut stall)
                .await
            {
                RoundOutcome::Continue => {
                    if let Some(record) = stopped_or_stalled(
                        &mut state,
                        &mut tool_context,
                        &stall,
                        self.limits.stall_threshold,
                    ) {
                        return Ok(record);
                    }
                }
                RoundOutcome::Terminal(ending, reply) => {
                    return Ok(state.into_record(ending, reply));
                }
                RoundOutcome::Failed(source) => {
                    let failure = RunFailure::Provider {
                        source,
                        trace: Box::new(state.into_trace()),
                    };
                    log_run_failure(&failure);
                    return Err(failure);
                }
            }
        }

        Ok(state.into_record(RunEnding::RoundsExhausted, None))
    }

    async fn run_round(
        &self,
        conversation: &[Message],
        state: &mut RunState,
        inputs: &RoundInputs<'_>,
        stall: &mut StallTracker,
    ) -> RoundOutcome {
        let request = self.build_request(conversation, &state.appended);
        let (completion, latency) = match self.stream_round(request, inputs.observer).await {
            Ok(pair) => pair,
            Err(source) => return RoundOutcome::Failed(source),
        };

        let summary = RoundSummary {
            usage: completion.usage,
            stop_reason: completion.stop_reason,
            latency,
        };

        if summary.stop_reason == StopReason::MaxTokens {
            return finish_truncated(state, completion.content, summary, inputs.observer).await;
        }

        let message = Message {
            role: Role::Assistant,
            content: completion.content,
        };
        if !message.requests_tools() {
            return finish_completed(state, message, summary, inputs.observer).await;
        }

        self.run_tool_round(state, message, summary, inputs, stall)
            .await
    }

    fn build_request(&self, conversation: &[Message], appended: &[Message]) -> CompletionRequest {
        let mut messages = Vec::with_capacity(1 + conversation.len() + appended.len());
        messages.extend(render_system_prompt(&self.sections));
        messages.extend(conversation.iter().cloned());
        messages.extend(appended.iter().cloned());
        CompletionRequest {
            messages,
            tools: self.tools.specs(),
            ..CompletionRequest::default()
        }
    }

    async fn stream_round(
        &self,
        request: CompletionRequest,
        observer: Option<&dyn RunObserver>,
    ) -> Result<(Completion, Duration), ProviderFailure> {
        let started = Instant::now();
        let mut events = self.model.stream(request).await?;
        let mut accumulator = CompletionAccumulator::new();
        while let Some(event) = events.next().await {
            let event = event?;
            if let Some(observer) = observer {
                observer.on_event(&event).await;
            }
            accumulator.push(event);
        }
        let completion = accumulator.finish()?;
        Ok((completion, started.elapsed()))
    }

    async fn run_tool_round(
        &self,
        state: &mut RunState,
        message: Message,
        summary: RoundSummary,
        inputs: &RoundInputs<'_>,
        stall: &mut StallTracker,
    ) -> RoundOutcome {
        let tool_uses: Vec<ToolUse> = message.tool_uses().into_iter().cloned().collect();
        state.appended.push(message);

        let mut results = Vec::with_capacity(tool_uses.len());
        let mut calls = Vec::with_capacity(tool_uses.len());
        let mut cancelled = false;

        for tool_use in &tool_uses {
            if inputs.cancellation_token.is_cancelled() {
                cancelled = true;
                break;
            }
            let (result, record) = self.dispatch_one(tool_use, inputs).await;
            stall.record(&tool_use.name, &tool_use.input);
            results.push(result);
            calls.push(record);
        }

        let results_message = Message::tool_results(results);
        push_round(state, results_message, summary, calls, inputs.observer).await;

        if cancelled {
            RoundOutcome::Terminal(RunEnding::Cancelled, None)
        } else {
            RoundOutcome::Continue
        }
    }

    async fn dispatch_one(
        &self,
        tool_use: &ToolUse,
        inputs: &RoundInputs<'_>,
    ) -> (ToolResult, ToolCallRecord) {
        let started = Instant::now();
        let outcome = self
            .tools
            .dispatch(&tool_use.name, tool_use.input.clone(), inputs.tool_context)
            .await;
        let latency = started.elapsed();

        let (result, failed) = tool_outcome_to_result(tool_use, outcome);

        let record = ToolCallRecord {
            name: tool_use.name.clone(),
            arguments: tool_use.input.clone(),
            failed,
            latency,
        };
        if let Some(observer) = inputs.observer {
            observer.on_tool_call(&record).await;
        }
        (result, record)
    }
}

fn tool_outcome_to_result(
    tool_use: &ToolUse,
    outcome: Result<String, ToolFailure>,
) -> (ToolResult, bool) {
    match outcome {
        Ok(content) => (
            ToolResult {
                tool_use_id: tool_use.id.clone(),
                content,
                is_error: false,
            },
            false,
        ),
        Err(failure) => {
            tracing::warn!(tool = %tool_use.name, error = %failure, "tool call failed");
            (failure.into_result(tool_use.id.clone()), true)
        }
    }
}

/// Builds an [`Agent`]. Obtained from [`Agent::builder`].
pub struct AgentBuilder {
    model: Model,
    tools: ToolSet,
    sections: Vec<SystemSection>,
    limits: Limits,
}

impl AgentBuilder {
    /// Appends one system-prompt section.
    #[must_use]
    pub fn section(mut self, section: impl Into<SystemSection>) -> Self {
        self.sections.push(section.into());
        self
    }

    /// Appends several system-prompt sections, in order.
    #[must_use]
    pub fn sections(mut self, sections: impl IntoIterator<Item = SystemSection>) -> Self {
        self.sections.extend(sections);
        self
    }

    /// Sets the round and stall limits. Defaults to [`Limits::default`].
    #[must_use]
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Finishes building the `Agent`.
    #[must_use]
    pub fn build(self) -> Agent {
        Agent {
            model: self.model,
            tools: self.tools,
            sections: self.sections,
            limits: self.limits,
        }
    }
}

/// What running one round produced.
enum RoundOutcome {
    /// The round dispatched tools without stopping the run; the loop should
    /// check for a stop request and a stall before continuing.
    Continue,
    /// The round produced a final ending on its own (a reply, a truncation,
    /// or a cancellation discovered mid-dispatch).
    Terminal(RunEnding, Option<String>),
    /// The model call failed.
    Failed(ProviderFailure),
}

/// Per-run inputs a round needs that do not change between rounds.
struct RoundInputs<'a> {
    tool_context: &'a ToolContext,
    cancellation_token: &'a CancellationToken,
    observer: Option<&'a dyn RunObserver>,
}

/// The parts of a completion a finished round's record is built from,
/// bundled so the functions that build one don't each need three separate
/// parameters.
struct RoundSummary {
    usage: Usage,
    stop_reason: StopReason,
    latency: Duration,
}

/// Accumulates what a run has produced so far.
#[derive(Default)]
struct RunState {
    appended: Vec<Message>,
    rounds: Vec<RoundRecord>,
}

impl RunState {
    fn into_trace(self) -> RunTrace {
        let total_usage = total_usage(&self.rounds);
        RunTrace {
            messages: self.appended,
            rounds: self.rounds,
            total_usage,
        }
    }

    fn into_record(self, ending: RunEnding, reply: Option<String>) -> RunRecord {
        RunRecord {
            ending,
            reply,
            trace: self.into_trace(),
        }
    }
}

/// Tracks consecutive, structurally identical dispatched tool calls in
/// dispatch order across rounds.
#[derive(Debug, Default)]
struct StallTracker {
    last: Option<(String, serde_json::Value)>,
    streak: u32,
}

impl StallTracker {
    /// Records one dispatched call, incrementing the streak if it repeats
    /// the previous call by name and structural argument equality, or
    /// resetting it to one otherwise.
    fn record(&mut self, name: &str, arguments: &serde_json::Value) {
        let repeats_last = self
            .last
            .as_ref()
            .is_some_and(|(last_name, last_arguments)| {
                last_name == name && last_arguments == arguments
            });
        self.streak = if repeats_last { self.streak + 1 } else { 1 };
        self.last = Some((name.to_owned(), arguments.clone()));
    }

    /// The repeated tool name and streak length, if the streak has reached
    /// `threshold`.
    fn tripped(&self, threshold: u32) -> Option<(String, u32)> {
        (self.streak >= threshold)
            .then(|| self.last.clone().map(|(name, _)| (name, self.streak)))
            .flatten()
    }
}

async fn finish_truncated(
    state: &mut RunState,
    content: Vec<Content>,
    summary: RoundSummary,
    observer: Option<&dyn RunObserver>,
) -> RoundOutcome {
    let content: Vec<Content> = content
        .into_iter()
        .filter(|block| !matches!(block, Content::ToolUse(_)))
        .collect();
    let message = Message {
        role: Role::Assistant,
        content,
    };
    let reply = non_empty(message.text_content());
    push_round(state, message, summary, Vec::new(), observer).await;
    RoundOutcome::Terminal(RunEnding::Truncated, reply)
}

async fn finish_completed(
    state: &mut RunState,
    message: Message,
    summary: RoundSummary,
    observer: Option<&dyn RunObserver>,
) -> RoundOutcome {
    let reply = non_empty(message.text_content());
    push_round(state, message, summary, Vec::new(), observer).await;
    RoundOutcome::Terminal(RunEnding::Completed, reply)
}

async fn push_round(
    state: &mut RunState,
    message: Message,
    summary: RoundSummary,
    tool_calls: Vec<ToolCallRecord>,
    observer: Option<&dyn RunObserver>,
) {
    let round = RoundRecord {
        usage: summary.usage,
        stop_reason: summary.stop_reason,
        latency: summary.latency,
        tool_calls,
    };
    if let Some(observer) = observer {
        observer.on_round(&round).await;
    }
    state.rounds.push(round);
    state.appended.push(message);
}

/// After a round that dispatched tools without ending the run on its own,
/// checks for a stop request and then a stall — in that order, matching the
/// loop's documented rule — and finishes the run's record if either fired.
fn stopped_or_stalled(
    state: &mut RunState,
    tool_context: &mut ToolContext,
    stall: &StallTracker,
    stall_threshold: u32,
) -> Option<RunRecord> {
    if let Some(stop_request) = tool_context.take_stop_request() {
        let reply = Some(stop_request.reply.clone());
        let state = std::mem::take(state);
        return Some(state.into_record(RunEnding::StopRequested(stop_request), reply));
    }
    if let Some((tool, count)) = stall.tripped(stall_threshold) {
        let state = std::mem::take(state);
        return Some(state.into_record(RunEnding::Stalled { tool, count }, None));
    }
    None
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn sum_optional(total: Option<u64>, value: Option<u64>) -> Option<u64> {
    total.zip(value).map(|(total, value)| total + value)
}

fn total_usage(rounds: &[RoundRecord]) -> Usage {
    let mut input_tokens = Some(0_u64);
    let mut output_tokens = Some(0_u64);
    for round in rounds {
        input_tokens = sum_optional(input_tokens, round.usage.input_tokens);
        output_tokens = sum_optional(output_tokens, round.usage.output_tokens);
    }
    Usage {
        input_tokens,
        output_tokens,
    }
}

fn validate_conversation(messages: &[Message]) -> Result<(), RunFailure> {
    if messages.iter().any(|message| message.role == Role::System) {
        return Err(invalid_conversation(
            "the conversation must not contain system messages — an Agent's sections are the \
             only system prompt",
        ));
    }
    validate_messages(messages).map_err(|failure| match failure {
        ProviderFailure::InvalidRequest(reason) => invalid_conversation(reason),
        other @ (ProviderFailure::Transport { .. }
        | ProviderFailure::Status { .. }
        | ProviderFailure::Decode { .. }
        | ProviderFailure::Configuration(_)) => invalid_conversation(other.to_string()),
    })
}

fn invalid_conversation(reason: impl Into<String>) -> RunFailure {
    RunFailure::InvalidConversation {
        reason: reason.into(),
    }
}

fn log_run_failure(failure: &RunFailure) {
    match failure {
        RunFailure::InvalidConversation { reason } => {
            tracing::error!(reason = %reason, "run failed: invalid conversation");
        }
        RunFailure::Provider { source, .. } => {
            tracing::error!(error = %source, "run failed: provider call failed");
        }
    }
}

#[cfg(test)]
#[path = "tests/run.rs"]
mod tests;
