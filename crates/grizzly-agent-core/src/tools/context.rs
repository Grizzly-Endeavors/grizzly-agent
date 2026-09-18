//! Per-run state shared between the turn loop and the tools it dispatches.

use std::sync::OnceLock;

use tokio_util::sync::CancellationToken;

/// A request, recorded by a tool, to end the run after the current round.
///
/// This is the "hand this to a human" exit: a tool that wants to end the run
/// records one on the [`ToolContext`] rather than returning a specially
/// shaped result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopRequest {
    /// What to tell the user.
    pub reply: String,
    /// Why the run is ending, for whoever handles it next.
    pub reason: String,
}

/// The typed side channel between a run's tools and the turn loop.
///
/// Created fresh by the loop for each run and passed by shared reference to
/// every tool call in that run. It carries the run's cancellation token and
/// a first-wins slot for a stop request: if more than one tool records one
/// in the same round, the first call to [`ToolContext::request_stop`] wins
/// and later calls are ignored.
#[derive(Debug)]
pub struct ToolContext {
    cancellation_token: CancellationToken,
    stop_request: OnceLock<StopRequest>,
}

impl ToolContext {
    /// Creates a fresh context for one run, carrying its cancellation token.
    #[must_use]
    pub fn new(cancellation_token: CancellationToken) -> Self {
        Self {
            cancellation_token,
            stop_request: OnceLock::new(),
        }
    }

    /// The run's cancellation token.
    ///
    /// Tools observe it to notice a cancelled run; the run's caller holds a
    /// clone and cancels it from outside.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Records a request to end the run after the current round.
    ///
    /// Returns whether this call recorded the request. `false` means another
    /// tool already recorded one first, and this call was ignored.
    #[must_use]
    pub fn request_stop(&self, reply: impl Into<String>, reason: impl Into<String>) -> bool {
        self.stop_request
            .set(StopRequest {
                reply: reply.into(),
                reason: reason.into(),
            })
            .is_ok()
    }

    /// Takes the recorded stop request, if any, leaving the slot empty.
    ///
    /// The turn loop calls this after each round to decide whether the run
    /// ends with `StopRequested`.
    #[must_use]
    pub fn take_stop_request(&mut self) -> Option<StopRequest> {
        self.stop_request.take()
    }
}

#[cfg(test)]
#[path = "tests/context.rs"]
mod tests;
