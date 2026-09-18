//! Drives [`ResponseEvalCase`]s against a [`Model`], one repeat at a time,
//! up to a concurrency limit shared across every case.

use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use grizzly_agent_core::{Completion, CompletionRequest, Content, Model};

use crate::aggregate::CaseAggregate;
use crate::case::DEFAULT_REPEATS;
use crate::report::{CaseReport, RepeatRecord};
use crate::verdict::Verdict;

use super::case::{CaseTimeout, ResponseEvalCase};

/// Runs [`ResponseEvalCase`]s against a [`Model`], producing one
/// [`CaseReport`] per case.
///
/// A provider failure or an elapsed timeout classifies a repeat as
/// [`crate::VerdictCategory::Unavailable`]; a parse failure classifies it as
/// [`crate::VerdictCategory::Unparseable`]. Every other repeat is handed to
/// the case's own scorer.
pub struct ResponseEvalRunner {
    model: Model,
    default_timeout: Option<Duration>,
    concurrency: usize,
    default_repeats: u32,
}

impl ResponseEvalRunner {
    /// Start building a runner over `model`, bounding concurrent calls to
    /// `concurrency` (at least 1, regardless of what is passed).
    #[must_use]
    pub fn builder(model: Model, concurrency: usize) -> ResponseEvalRunnerBuilder {
        ResponseEvalRunnerBuilder {
            model,
            default_timeout: None,
            concurrency: concurrency.max(1),
            default_repeats: DEFAULT_REPEATS,
        }
    }

    /// Run every repeat of every case in `cases`, in the order given.
    ///
    /// Repeats run concurrently up to this runner's concurrency limit, as one
    /// bound shared across every case rather than one per case.
    pub async fn run<C: ResponseEvalCase>(&self, cases: &[C]) -> Vec<CaseReport> {
        let mut work: Vec<(usize, &C)> = Vec::new();
        for (case_index, case) in cases.iter().enumerate() {
            let repeats = case.meta().effective_repeats(self.default_repeats);
            for _ in 0..repeats {
                work.push((case_index, case));
            }
        }

        let results: Vec<(usize, RepeatRecord)> = stream::iter(
            work.into_iter()
                .map(|(case_index, case)| async move { (case_index, self.run_one(case).await) }),
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

    /// Run one repeat of `case`: build its request, call the model under its
    /// resolved timeout, and score the reply.
    async fn run_one<C: ResponseEvalCase>(&self, case: &C) -> RepeatRecord {
        let request = case.build_request();
        let timeout = self.effective_timeout(case.timeout());
        let start = Instant::now();
        let outcome = self.call(request, timeout).await;
        let latency = start.elapsed();

        match outcome {
            Err(detail) => RepeatRecord::new(Verdict::unavailable(detail).with_latency(latency)),
            Ok(completion) => score_reply(case, &completion, latency),
        }
    }

    async fn call(
        &self,
        request: CompletionRequest,
        timeout: Option<Duration>,
    ) -> Result<Completion, String> {
        let call = self.model.complete(request);
        match timeout {
            None => call.await.map_err(|failure| failure.to_string()),
            Some(bound) => match tokio::time::timeout(bound, call).await {
                Ok(result) => result.map_err(|failure| failure.to_string()),
                Err(_elapsed) => Err(format!("no reply within {} s", bound.as_secs_f64())),
            },
        }
    }

    fn effective_timeout(&self, case_timeout: CaseTimeout) -> Option<Duration> {
        match case_timeout {
            CaseTimeout::Default => self.default_timeout,
            CaseTimeout::Custom(duration) => Some(duration),
            CaseTimeout::None => None,
        }
    }
}

/// Parse and score `completion` against `case`'s expectation, keeping the
/// reply's raw text in the record's detail whenever the verdict misses.
fn score_reply<C: ResponseEvalCase>(
    case: &C,
    completion: &Completion,
    latency: Duration,
) -> RepeatRecord {
    let usage = completion.usage;
    let raw_reply = reply_text(completion);
    match case.parse(completion) {
        Err(detail) => RepeatRecord::new(
            Verdict::unparseable(detail)
                .with_latency(latency)
                .with_usage(usage),
        )
        .with_detail(serde_json::json!({ "raw_reply": raw_reply })),
        Ok(answer) => {
            let verdict = case.score(&answer).with_latency(latency).with_usage(usage);
            let record = RepeatRecord::new(verdict);
            if record.verdict.passed() {
                record
            } else {
                record.with_detail(serde_json::json!({ "raw_reply": raw_reply }))
            }
        }
    }
}

/// The reply's visible text: every [`Content::Text`] block joined by
/// newlines, excluding reasoning and tool blocks.
fn reply_text(completion: &Completion) -> String {
    completion
        .content
        .iter()
        .filter_map(|block| match block {
            Content::Text(text) => Some(text.as_str()),
            Content::Reasoning { .. } | Content::ToolUse(_) | Content::ToolResult(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Builds a [`ResponseEvalRunner`]. Obtained from [`ResponseEvalRunner::builder`].
pub struct ResponseEvalRunnerBuilder {
    model: Model,
    default_timeout: Option<Duration>,
    concurrency: usize,
    default_repeats: u32,
}

impl ResponseEvalRunnerBuilder {
    /// The timeout a case falls back to when it does not set its own
    /// ([`CaseTimeout::Default`]). Unset, such a case never times out.
    #[must_use]
    pub fn default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
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
    pub fn build(self) -> ResponseEvalRunner {
        ResponseEvalRunner {
            model: self.model,
            default_timeout: self.default_timeout,
            concurrency: self.concurrency,
            default_repeats: self.default_repeats,
        }
    }
}

#[cfg(test)]
#[path = "tests/runner.rs"]
mod tests;
