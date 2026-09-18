//! `ResponseEval` and `AgentEval` over a shared scoring and report core.
//!
//! `ResponseEval` answers *how does a model do at this single call?* and
//! `AgentEval` answers *how does this agent harness do at this task?* Both are
//! provider-blind — the provider is chosen when the [`grizzly_agent_core::Model`]
//! or `Agent` is built — and both feed the shared core in this crate: case
//! metadata, verdicts, aggregation, and the report.
//!
//! The shared core is public so a consumer with a bespoke runner — one that
//! does not fit either shape — can still build [`Verdict`]s of its own and
//! feed them into [`CaseAggregate::aggregate`] and a [`Report`], reusing the
//! same threshold arithmetic and the same document format `ResponseEval` and
//! `AgentEval` produce.
//!
//! This crate depends on `grizzly-agent-core` for the conversation and model
//! types, and on nothing that talks to a wire — no provider dependency, so a
//! consumer testing a bespoke runner never pulls in HTTP.

mod aggregate;
mod case;
mod check;
mod dir;
mod report;
mod response_eval;
mod verdict;

pub use crate::aggregate::CaseAggregate;
pub use crate::case::{CaseMeta, DEFAULT_MIN_PASS_RATE, DEFAULT_REPEATS};
pub use crate::check::CheckResult;
pub use crate::dir::{InvocationDir, InvocationDirError, REPORT_FILE};
pub use crate::report::{
    CaseReport, REPORT_SCHEMA_VERSION, RepeatRecord, Report, SuiteResult, render_summary,
};
pub use crate::response_eval::{
    CaseTimeout, ResponseEvalCase, ResponseEvalRunner, ResponseEvalRunnerBuilder,
};
pub use crate::verdict::{Verdict, VerdictCategory};
