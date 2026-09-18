//! An agent-harness eval case: build a fresh `Agent`, set up its
//! environment, check it after the run, and score the result.

use grizzly_agent_core::{Agent, Message, RunRecord};

use crate::case::CaseMeta;
use crate::check::CheckResult;
use crate::verdict::Verdict;

/// One agent harness measured against a task with a hidden, post-run oracle.
///
/// Unlike [`crate::ResponseEvalCase`], which scores a single model call,
/// this scores a whole [`Agent::run`]: the case sets up an environment the
/// agent acts on, hands the agent its task as a conversation, and checks the
/// environment afterward with assertions the agent never saw — for example,
/// hidden tests a coding task must pass, run only once the agent has
/// stopped touching the scratch directory those tests run against.
///
/// `Self::Environment` is the per-repeat state [`AgentEvalCase::set_up`]
/// produces and [`AgentEvalCase::check`] consumes — typically a handle to a
/// scratch directory or other resource unique to that repeat.
#[async_trait::async_trait]
pub trait AgentEvalCase: Send + Sync {
    /// Per-repeat state set up produces and check consumes.
    type Environment: Send + Sync;

    /// This case's metadata: name, repeats, threshold, canary.
    fn meta(&self) -> &CaseMeta;

    /// Build a fresh `Agent` for one repeat.
    ///
    /// Called once per repeat rather than once per case, so a case may give
    /// each repeat its own harness — for example one bound to a tool that
    /// only that repeat's environment should be able to reach.
    fn build_agent(&self) -> Agent;

    /// Set up the environment this repeat's agent will act on, returning a
    /// handle to it together with the conversation — the task — to pass to
    /// `Agent::run`.
    ///
    /// # Errors
    /// Returns a human-readable detail when set up cannot complete. The
    /// runner turns this into a [`crate::VerdictCategory::Failed`] verdict
    /// carrying it as the reason, without running the agent at all.
    async fn set_up(&self) -> Result<(Self::Environment, Vec<Message>), String>;

    /// Check `environment` after the run completes: assertions the agent
    /// never saw, evaluated against whatever `set_up` seeded and the run may
    /// have changed.
    ///
    /// # Errors
    /// Returns a human-readable detail when the checks themselves cannot be
    /// run at all — not when a check merely fails, which is a
    /// [`CheckResult::failed`] inside the returned `Vec`. The runner turns
    /// an `Err` here into a [`crate::VerdictCategory::Failed`] verdict
    /// carrying it as the reason, without calling `score`.
    async fn check(&self, environment: &Self::Environment) -> Result<Vec<CheckResult>, String>;

    /// Score a finished run against its check results.
    fn score(&self, record: &RunRecord, checks: &[CheckResult]) -> Verdict;
}
