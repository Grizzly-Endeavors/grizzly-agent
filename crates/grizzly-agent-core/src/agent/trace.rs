//! What a run produced: the messages it appended, what happened each round,
//! and how it finished.

use std::time::Duration;

use crate::completion::{StopReason, Usage};
use crate::message::Message;
use crate::tools::StopRequest;

/// One tool call a round dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRecord {
    /// The tool's registered name.
    pub name: String,
    /// The arguments it was called with.
    pub arguments: serde_json::Value,
    /// Whether the call failed.
    pub failed: bool,
    /// How long the call took.
    pub latency: Duration,
}

/// What happened in one round of the loop: one model call, and every tool
/// call it triggered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundRecord {
    /// Usage the model reported for this round's completion.
    pub usage: Usage,
    /// The round's classified stop reason.
    pub stop_reason: StopReason,
    /// How long the model call took, from request to a finished completion.
    pub latency: Duration,
    /// Every tool call the round dispatched, in dispatch order.
    pub tool_calls: Vec<ToolCallRecord>,
}

/// The progress of a run: what happened, independent of how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunTrace {
    /// Messages appended during the run — assistant replies and tool
    /// results — in the order they were produced. Does not include the
    /// conversation the caller passed to `Agent::run`; a caller resuming the
    /// conversation appends these onto it.
    pub messages: Vec<Message>,
    /// Every round the run completed, in order.
    pub rounds: Vec<RoundRecord>,
    /// Usage summed across every completed round. A field is `None` if any
    /// round reported it unknown.
    pub total_usage: Usage,
}

/// How a run ended.
///
/// Closed rather than [`non_exhaustive`](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute):
/// this is the loop's whole vocabulary of stopping points, so a caller or a
/// scorer matching on it exhaustively is never surprised by a new variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnding {
    /// The model answered without requesting tools.
    Completed,
    /// The model hit its output-token limit before finishing.
    Truncated,
    /// A tool asked for the run to end after this round.
    StopRequested(StopRequest),
    /// The run reached its round limit without reaching another ending.
    RoundsExhausted,
    /// The same tool call repeated enough consecutive times to trip the
    /// stall threshold.
    Stalled {
        /// The tool that repeated.
        tool: String,
        /// How many consecutive times it repeated.
        count: u32,
    },
    /// The run's cancellation token was set before further work began.
    Cancelled,
}

/// A finished run: its ending, its final reply, and everything it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    /// How the run ended.
    pub ending: RunEnding,
    /// The final reply text, when the ending produced one.
    pub reply: Option<String>,
    /// Everything the run did.
    pub trace: RunTrace,
}

#[cfg(test)]
#[path = "tests/trace.rs"]
mod tests;
