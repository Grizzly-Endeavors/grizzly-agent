//! Folding a case's repeats into the one summary an operator reads.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::case::CaseMeta;
use crate::verdict::{Verdict, VerdictCategory};

/// Everything an operator reads about one case at a glance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseAggregate {
    /// The case's name.
    pub name: String,
    /// Whether this case is a canary.
    pub canary: bool,
    /// Repeats actually run.
    pub repeats: u32,
    /// Repeats that passed.
    pub passes: u32,
    /// `passes / repeats`, or 0.0 when no repeat ran.
    pub pass_rate: f64,
    /// The pass rate this case had to reach.
    pub threshold: f64,
    /// Whether `pass_rate` met `threshold`. Always `false` when no repeat
    /// ran, since an empty run measured nothing.
    pub met_threshold: bool,
    /// Repeats that answered but got it wrong.
    pub wrong: u32,
    /// Repeats the parser could not read.
    pub unparseable: u32,
    /// Repeats that got no answer at all.
    pub unavailable: u32,
    /// Repeats that measured nothing due to harness trouble.
    pub failed: u32,
    /// The mean latency across repeats that reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_latency: Option<Duration>,
    /// The mean input token count across repeats that reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_input_tokens: Option<u64>,
    /// The mean output token count across repeats that reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean_output_tokens: Option<u64>,
}

impl CaseAggregate {
    /// Fold `verdicts` — one case's repeats — into its aggregate, deciding
    /// whether the case met its threshold.
    #[must_use]
    pub fn aggregate(meta: &CaseMeta, verdicts: &[Verdict]) -> Self {
        let repeats = u32::try_from(verdicts.len()).unwrap_or(u32::MAX);
        let passes = count(verdicts, VerdictCategory::Pass);
        let threshold = meta.threshold();
        let pass_rate = if repeats == 0 {
            0.0
        } else {
            f64::from(passes) / f64::from(repeats)
        };
        Self {
            name: meta.name.clone(),
            canary: meta.canary,
            repeats,
            passes,
            pass_rate,
            threshold,
            met_threshold: repeats > 0 && pass_rate >= threshold,
            wrong: count(verdicts, VerdictCategory::Wrong),
            unparseable: count(verdicts, VerdictCategory::Unparseable),
            unavailable: count(verdicts, VerdictCategory::Unavailable),
            failed: count(verdicts, VerdictCategory::Failed),
            mean_latency: mean_duration(verdicts.iter().filter_map(|verdict| verdict.latency)),
            mean_input_tokens: mean_u64(
                verdicts
                    .iter()
                    .filter_map(|verdict| verdict.usage.and_then(|usage| usage.input_tokens)),
            ),
            mean_output_tokens: mean_u64(
                verdicts
                    .iter()
                    .filter_map(|verdict| verdict.usage.and_then(|usage| usage.output_tokens)),
            ),
        }
    }
}

fn count(verdicts: &[Verdict], category: VerdictCategory) -> u32 {
    let matching = verdicts
        .iter()
        .filter(|verdict| verdict.category == category)
        .count();
    u32::try_from(matching).unwrap_or(u32::MAX)
}

/// Integer mean, so no reported average token count is a lossy float.
fn mean_u64(values: impl Iterator<Item = u64>) -> Option<u64> {
    let mut total: u64 = 0;
    let mut seen: u64 = 0;
    for value in values {
        total = total.saturating_add(value);
        seen += 1;
    }
    total.checked_div(seen)
}

fn mean_duration(values: impl Iterator<Item = Duration>) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut seen: u32 = 0;
    for value in values {
        total = total.saturating_add(value);
        seen += 1;
    }
    total.checked_div(seen)
}

#[cfg(test)]
#[path = "tests/aggregate.rs"]
mod tests;
