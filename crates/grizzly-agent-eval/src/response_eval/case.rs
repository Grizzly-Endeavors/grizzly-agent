//! A single-call eval case: build the request, parse the reply, score it.

use std::time::Duration;

use grizzly_agent_core::{Completion, CompletionRequest};

use crate::case::CaseMeta;
use crate::verdict::Verdict;

/// How a case's per-call timeout is resolved against the runner's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaseTimeout {
    /// Use the runner's default timeout, whatever it is set to (including
    /// none).
    #[default]
    Default,
    /// Use this duration regardless of the runner's default.
    Custom(Duration),
    /// Never time this case out, regardless of the runner's default.
    None,
}

/// One model call measured against a known-right answer.
///
/// The request builder and parser are meant to be the consumer's
/// **production** ones — the eval deliberately bypasses any production
/// fallback that would turn a dead endpoint into a plausible default answer,
/// which is why [`ResponseEvalRunner`](crate::ResponseEvalRunner) classifies
/// a call that failed outright as [`crate::VerdictCategory::Unavailable`]
/// rather than handing your fallback's answer to [`ResponseEvalCase::score`].
pub trait ResponseEvalCase: Send + Sync {
    /// The type a successful call parses into.
    type Answer;

    /// This case's metadata: name, repeats, threshold, canary.
    fn meta(&self) -> &CaseMeta;

    /// Build the request for this case's call.
    fn build_request(&self) -> CompletionRequest;

    /// Parse `completion` into this case's answer type.
    ///
    /// # Errors
    /// Returns a human-readable detail when the reply cannot be read — the
    /// runner turns this into a [`crate::VerdictCategory::Unparseable`]
    /// verdict carrying it as the reason.
    fn parse(&self, completion: &Completion) -> Result<Self::Answer, String>;

    /// Score a parsed answer against this case's expectation.
    fn score(&self, answer: &Self::Answer) -> Verdict;

    /// This case's timeout, given the runner's own default.
    ///
    /// The default implementation defers to the runner
    /// ([`CaseTimeout::Default`]); override to set the case's own duration or
    /// to disable the timeout for this case.
    fn timeout(&self) -> CaseTimeout {
        CaseTimeout::default()
    }
}

#[cfg(test)]
#[path = "tests/case.rs"]
mod tests;
