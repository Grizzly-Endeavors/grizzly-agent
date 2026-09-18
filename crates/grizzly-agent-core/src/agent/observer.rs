//! The hook a run optionally notifies of its progress: every streamed
//! event, every finished round, and every finished tool call — so a live UI
//! and telemetry are structural rather than something a caller wraps around
//! the loop by hand.

use crate::agent::trace::{RoundRecord, ToolCallRecord};
use crate::completion::CompletionEvent;

/// Notified of a run's progress as it happens.
///
/// The observer cannot change the run: every method takes a shared
/// reference to the observer and returns nothing. Default implementations
/// are no-ops, so a consumer implements only the methods it cares about.
#[async_trait::async_trait]
pub trait RunObserver: Send + Sync {
    /// Called for every event as it streams from the model, in arrival
    /// order.
    async fn on_event(&self, _event: &CompletionEvent) {}

    /// Called once a round finishes: the model replied and, if it asked for
    /// tools, every tool in the round has been dispatched.
    async fn on_round(&self, _round: &RoundRecord) {}

    /// Called once a single tool call finishes, whether it succeeded or
    /// failed.
    async fn on_tool_call(&self, _call: &ToolCallRecord) {}
}

#[cfg(test)]
#[path = "tests/observer.rs"]
mod tests;
