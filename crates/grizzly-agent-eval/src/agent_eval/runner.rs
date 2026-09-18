//! Drives [`AgentEvalCase`]s through set up → [`Agent::run`] → check →
//! score, one repeat at a time by default and up to a configurable limit.

use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use grizzly_agent_core::{Message, RunEnding, RunFailure, RunRecord, RunTrace, Usage};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::aggregate::CaseAggregate;
use crate::case::DEFAULT_REPEATS;
use crate::check::CheckResult;
use crate::report::{CaseReport, RepeatRecord};
use crate::verdict::Verdict;

use super::case::AgentEvalCase;

/// Runs [`AgentEvalCase`]s, producing one [`CaseReport`] per case.
///
/// A [`RunFailure::Provider`] classifies a repeat as
/// [`crate::VerdictCategory::Unavailable`] — the endpoint broke, not the
/// harness, so check and score are skipped and the failure's trace is kept
/// in the report detail. A [`RunFailure::InvalidConversation`], or an error
/// from set up or check, classifies a repeat as
/// [`crate::VerdictCategory::Failed`] — the case itself is broken. Every
/// other repeat is handed to the case's own scorer.
pub struct AgentEvalRunner {
    concurrency: usize,
    default_repeats: u32,
}

impl AgentEvalRunner {
    /// Starts building a runner, defaulting to one repeat at a time.
    #[must_use]
    pub fn builder() -> AgentEvalRunnerBuilder {
        AgentEvalRunnerBuilder {
            concurrency: 1,
            default_repeats: DEFAULT_REPEATS,
        }
    }

    /// Run every repeat of every case in `cases`, in the order given.
    ///
    /// `AgentEval` repeats are heavy and usually share an endpoint, so they
    /// run one at a time unless [`AgentEvalRunnerBuilder::concurrency`]
    /// raises the limit — one bound shared across every case rather than one
    /// per case.
    pub async fn run<C: AgentEvalCase>(&self, cases: &[C]) -> Vec<CaseReport> {
        let mut work: Vec<(usize, &C)> = Vec::new();
        for (case_index, case) in cases.iter().enumerate() {
            let repeats = case.meta().effective_repeats(self.default_repeats);
            for _ in 0..repeats {
                work.push((case_index, case));
            }
        }

        let results: Vec<(usize, RepeatRecord)> = stream::iter(
            work.into_iter()
                .map(|(case_index, case)| async move { (case_index, run_one(case).await) }),
        )
        .buffer_unordered(self.concurrency)
        .collect()
        .await;

        let mut per_case: Vec<Vec<RepeatRecord>> = cases.iter().map(|_| Vec::new()).collect();
        for (case_index, record) in results {
            if let Some(slot) = per_case.get_mut(case_index) {
                slot.push(record);
            }
        }

        cases
            .iter()
            .zip(per_case)
            .map(|(case, repeats)| {
                let verdicts: Vec<Verdict> = repeats
                    .iter()
                    .map(|record| record.verdict.clone())
                    .collect();
                CaseReport {
                    aggregate: CaseAggregate::aggregate(case.meta(), &verdicts),
                    repeats,
                }
            })
            .collect()
    }
}

/// Run one repeat of `case`: set up its environment, run its agent, check
/// the environment, and score the result.
async fn run_one<C: AgentEvalCase>(case: &C) -> RepeatRecord {
    let start = Instant::now();
    let agent = case.build_agent();

    let (environment, conversation) = match case.set_up().await {
        Ok(pair) => pair,
        Err(detail) => {
            return RepeatRecord::new(Verdict::failed(detail).with_latency(start.elapsed()));
        }
    };

    let outcome = agent
        .run(conversation, CancellationToken::new(), case.observer())
        .await;
    let latency = start.elapsed();

    match outcome {
        Err(RunFailure::Provider { source, trace }) => {
            RepeatRecord::new(Verdict::unavailable(source.to_string()).with_latency(latency))
                .with_detail(provider_failure_detail(&trace))
        }
        Err(RunFailure::InvalidConversation { reason }) => {
            RepeatRecord::new(Verdict::failed(reason).with_latency(latency))
        }
        Ok(record) => score_run(case, &environment, record, latency).await,
    }
}

/// Check the environment and, if that succeeds, score the run — building
/// the repeat's report detail either way.
async fn score_run<C: AgentEvalCase>(
    case: &C,
    environment: &C::Environment,
    record: RunRecord,
    latency: Duration,
) -> RepeatRecord {
    match case.check(environment).await {
        Err(detail) => RepeatRecord::new(Verdict::failed(detail).with_latency(latency))
            .with_detail(run_detail(&record, &[], true)),
        Ok(checks) => {
            let usage = record.trace.total_usage;
            let verdict = case
                .score(&record, &checks)
                .with_latency(latency)
                .with_usage(usage);
            let include_transcript = !verdict.passed();
            let detail = run_detail(&record, &checks, include_transcript);
            RepeatRecord::new(verdict).with_detail(detail)
        }
    }
}

/// One repeat's report detail: the run's ending, total usage, round count,
/// and check results, with the full transcript kept only when
/// `include_transcript` — set for every miss, so a wrong or failed repeat is
/// diagnosable without a rerun.
#[derive(Debug, Serialize)]
struct RepeatDetail<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    ending: Option<EndingDetail>,
    total_usage: Usage,
    round_count: usize,
    checks: &'a [CheckResult],
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript: Option<&'a [Message]>,
}

/// [`RunEnding`] as it appears in the report — [`RunEnding`] itself carries
/// no `Serialize` impl, since core has no reason to depend on `serde` for it.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EndingDetail {
    Completed,
    Truncated,
    StopRequested { reply: String, reason: String },
    RoundsExhausted,
    Stalled { tool: String, count: u32 },
    Cancelled,
}

impl From<&RunEnding> for EndingDetail {
    fn from(ending: &RunEnding) -> Self {
        match ending {
            RunEnding::Completed => Self::Completed,
            RunEnding::Truncated => Self::Truncated,
            RunEnding::StopRequested(stop_request) => Self::StopRequested {
                reply: stop_request.reply.clone(),
                reason: stop_request.reason.clone(),
            },
            RunEnding::RoundsExhausted => Self::RoundsExhausted,
            RunEnding::Stalled { tool, count } => Self::Stalled {
                tool: tool.clone(),
                count: *count,
            },
            RunEnding::Cancelled => Self::Cancelled,
        }
    }
}

fn run_detail(
    record: &RunRecord,
    checks: &[CheckResult],
    include_transcript: bool,
) -> serde_json::Value {
    let detail = RepeatDetail {
        ending: Some(EndingDetail::from(&record.ending)),
        total_usage: record.trace.total_usage,
        round_count: record.trace.rounds.len(),
        checks,
        transcript: include_transcript.then_some(record.trace.messages.as_slice()),
    };
    serde_json::to_value(&detail).unwrap_or(serde_json::Value::Null)
}

/// The detail for a [`RunFailure::Provider`] repeat: no [`RunEnding`], since
/// the run never reached one, but every round completed before the failure —
/// kept in full, as the design requires, since this classifies as
/// [`crate::VerdictCategory::Unavailable`] and is always a miss.
fn provider_failure_detail(trace: &RunTrace) -> serde_json::Value {
    let detail = RepeatDetail {
        ending: None,
        total_usage: trace.total_usage,
        round_count: trace.rounds.len(),
        checks: &[],
        transcript: Some(trace.messages.as_slice()),
    };
    serde_json::to_value(&detail).unwrap_or(serde_json::Value::Null)
}

/// Builds an [`AgentEvalRunner`]. Obtained from [`AgentEvalRunner::builder`].
pub struct AgentEvalRunnerBuilder {
    concurrency: usize,
    default_repeats: u32,
}

impl AgentEvalRunnerBuilder {
    /// Bounds how many repeats run at once, across every case. Defaults to
    /// 1 and is floored at 1 regardless of what is passed — `AgentEval`
    /// repeats are heavy and usually share an endpoint, so sequential is the
    /// safe default.
    #[must_use]
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Repeats a case runs when it does not set its own count. Defaults to
    /// [`DEFAULT_REPEATS`].
    #[must_use]
    pub fn default_repeats(mut self, repeats: u32) -> Self {
        self.default_repeats = repeats;
        self
    }

    /// Finish building the runner.
    #[must_use]
    pub fn build(self) -> AgentEvalRunner {
        AgentEvalRunner {
            concurrency: self.concurrency,
            default_repeats: self.default_repeats,
        }
    }
}

#[cfg(test)]
#[path = "tests/runner.rs"]
mod tests;
