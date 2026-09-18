//! A scripted [`Provider`] for testing anything built on [`crate::Model`]
//! without a network.
//!
//! Compiled whenever this crate's own tests run, and additionally exposed to
//! consumers behind the `test-support` feature — every consumer needs this to
//! test anything built on a model, so it is part of the product rather than
//! an internal test helper.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::{StreamExt, stream};

use crate::completion::{Completion, CompletionEvent};
use crate::error::ProviderFailure;
use crate::message::Content;
use crate::provider::{CompletionStream, Provider};
use crate::request::CompletionRequest;

/// One scripted answer to a call the [`ScriptedProvider`] will serve.
#[derive(Debug)]
pub enum ScriptedResponse {
    /// A whole completion, emitted as a well-formed event sequence: a delta
    /// per text or reasoning block, a tool-use start and one arguments delta
    /// per tool call, usage if any field is known, then `Finished`.
    Completion(Completion),
    /// An explicit sequence of stream items, replayed verbatim — including
    /// any `Err`, to script a stream that breaks partway through.
    Events(Vec<Result<CompletionEvent, ProviderFailure>>),
    /// A failure before the stream opens at all.
    PreStreamFailure(ProviderFailure),
}

#[derive(Default)]
struct ScriptedProviderState {
    queue: VecDeque<ScriptedResponse>,
    served: u32,
    requests: Vec<CompletionRequest>,
    model_ids: Vec<String>,
}

/// A [`Provider`] that plays back a queued sequence of [`ScriptedResponse`]s
/// and records every request it received.
///
/// Cheap to clone: clones share the same queue and recorded requests, so a
/// test can hand one clone to a [`crate::Model`] and inspect
/// [`ScriptedProvider::requests`] on another afterward.
#[derive(Clone, Default)]
pub struct ScriptedProvider {
    state: Arc<Mutex<ScriptedProviderState>>,
}

impl ScriptedProvider {
    /// A provider that serves `responses` in order, one per call.
    #[must_use]
    pub fn new(responses: impl IntoIterator<Item = ScriptedResponse>) -> Self {
        let provider = Self::default();
        lock_state(&provider.state).queue.extend(responses);
        provider
    }

    /// Every request this provider has received, in call order.
    #[must_use]
    pub fn requests(&self) -> Vec<CompletionRequest> {
        lock_state(&self.state).requests.clone()
    }

    /// The model id passed to each call, in call order — index-aligned with
    /// [`Self::requests`], so a test can assert which model each request
    /// named on the wire.
    #[must_use]
    pub fn model_ids(&self) -> Vec<String> {
        lock_state(&self.state).model_ids.clone()
    }

    /// How many calls this provider has served.
    #[must_use]
    pub fn calls_served(&self) -> u32 {
        lock_state(&self.state).served
    }
}

/// Locks `state`, recovering it if an earlier panic poisoned the mutex rather
/// than panicking again here — a second panic on top of the first would only
/// obscure whichever assertion failed originally.
fn lock_state(state: &Mutex<ScriptedProviderState>) -> MutexGuard<'_, ScriptedProviderState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[async_trait::async_trait]
impl Provider for ScriptedProvider {
    async fn complete(
        &self,
        model: &str,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure> {
        let response = {
            let mut state = lock_state(&self.state);
            state.requests.push(request);
            state.model_ids.push(model.to_owned());
            let Some(response) = state.queue.pop_front() else {
                let served = state.served;
                return Err(ProviderFailure::Configuration(format!(
                    "scripted provider script exhausted after serving {served} call(s)"
                )));
            };
            state.served += 1;
            response
        };

        match response {
            ScriptedResponse::PreStreamFailure(failure) => Err(failure),
            ScriptedResponse::Events(events) => Ok(stream::iter(events).boxed()),
            ScriptedResponse::Completion(completion) => {
                Ok(stream::iter(events_for(completion)).boxed())
            }
        }
    }
}

fn events_for(completion: Completion) -> Vec<Result<CompletionEvent, ProviderFailure>> {
    let mut events: Vec<Result<CompletionEvent, ProviderFailure>> = completion
        .content
        .into_iter()
        .flat_map(events_for_block)
        .map(Ok)
        .collect();

    if completion.usage.input_tokens.is_some() || completion.usage.output_tokens.is_some() {
        events.push(Ok(CompletionEvent::Usage(completion.usage)));
    }
    events.push(Ok(CompletionEvent::Finished {
        stop_reason: completion.stop_reason,
        raw_stop_reason: completion.raw_stop_reason,
        model: completion.model,
    }));
    events
}

fn events_for_block(block: Content) -> Vec<CompletionEvent> {
    match block {
        Content::Text(text) => vec![CompletionEvent::TextDelta(text)],
        Content::Reasoning { text, signature: _ } => vec![CompletionEvent::ReasoningDelta(text)],
        Content::ToolUse(tool_use) => vec![
            CompletionEvent::ToolUseStart {
                id: tool_use.id.clone(),
                name: tool_use.name,
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: tool_use.id,
                fragment: tool_use.input.to_string(),
            },
        ],
        Content::ToolResult(_) => Vec::new(),
    }
}

#[cfg(test)]
#[path = "tests/test_support.rs"]
mod tests;
