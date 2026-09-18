//! The eval report: one JSON document per invocation, and the plain-text
//! summary rendered from it.
//!
//! The document is the product — comparing two invocations means reading two
//! reports, so everything needed to explain a verdict belongs here. A repeat
//! carries a consumer-supplied JSON detail, kept sparse for a pass and full
//! for a miss, so a wrong or unparseable answer is diagnosable without a
//! rerun.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::aggregate::CaseAggregate;
use crate::verdict::Verdict;

/// Schema version of the eval report document.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// One repeat as the report records it: its verdict, and a consumer-supplied
/// detail for diagnosing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepeatRecord {
    /// This repeat's verdict.
    pub verdict: Verdict,
    /// Consumer-supplied detail, kept sparse for a pass and full enough to
    /// diagnose a miss without a rerun (`serde_json::Value::Null` when the
    /// consumer has nothing to add).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub detail: serde_json::Value,
}

impl RepeatRecord {
    /// A repeat record with no detail.
    #[must_use]
    pub fn new(verdict: Verdict) -> Self {
        Self {
            verdict,
            detail: serde_json::Value::Null,
        }
    }

    /// This record, carrying `detail`.
    #[must_use]
    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = detail;
        self
    }
}

/// One case: its aggregate, and every repeat behind it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseReport {
    /// The case's folded summary.
    pub aggregate: CaseAggregate,
    /// Every repeat that went into the aggregate, in run order.
    pub repeats: Vec<RepeatRecord>,
}

/// Whether every case in a report met its threshold.
///
/// A consumer maps this to a process exit code; this crate has no opinion on
/// which code that should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuiteResult {
    /// `true` when every case's `met_threshold` is `true`.
    pub all_met: bool,
}

/// One invocation's report: the schema version, when and how it ran, and
/// every case it measured.
///
/// Assemble one with [`Report::start`] (a fresh invocation id and the
/// current time, stamped for you) or, when a consumer is also creating an
/// [`crate::InvocationDir`] for this invocation, [`crate::InvocationDir::report`]
/// so the two share an id. Reach for [`Report::new`] only when the
/// invocation id or start time must come from somewhere else — replaying a
/// past invocation, for instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// [`REPORT_SCHEMA_VERSION`] at the time this report was written.
    pub schema_version: u32,
    /// This invocation's id. UUID v7, so reports sort by time.
    pub invocation_id: Uuid,
    /// When this invocation started.
    pub started_at: Timestamp,
    /// Consumer-supplied settings — models, filters, and anything else
    /// needed for the report to explain itself — recorded as-is.
    pub settings: serde_json::Value,
    /// Every case this invocation measured.
    pub cases: Vec<CaseReport>,
}

impl Report {
    /// Assemble a report for one invocation.
    #[must_use]
    pub fn new(
        invocation_id: Uuid,
        started_at: Timestamp,
        settings: serde_json::Value,
        cases: Vec<CaseReport>,
    ) -> Self {
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            invocation_id,
            started_at,
            settings,
            cases,
        }
    }

    /// Assemble a report stamped with a fresh UUID v7 invocation id and the
    /// current time, so a consumer scoring cases needs neither `uuid` nor
    /// `jiff` as its own dependency just to produce a report.
    ///
    /// When this invocation also has an [`crate::InvocationDir`], prefer
    /// [`crate::InvocationDir::report`] instead, so the report's
    /// `invocation_id` matches the directory it gets written to.
    #[must_use]
    pub fn start(settings: serde_json::Value, cases: Vec<CaseReport>) -> Self {
        Self::new(Uuid::now_v7(), Timestamp::now(), settings, cases)
    }

    /// Whether every case in this report met its threshold.
    #[must_use]
    pub fn suite_result(&self) -> SuiteResult {
        SuiteResult {
            all_met: self.cases.iter().all(|case| case.aggregate.met_threshold),
        }
    }
}

const MIN_NAME_WIDTH: usize = 34;

/// Render the plain-text summary table for `report`.
///
/// The case column is sized to the longest name it has to hold: a fixed
/// width silently swallows the gap to the next column as soon as one case
/// outgrows it, and case names are consumer data that keeps growing.
#[must_use]
pub fn render_summary(report: &Report) -> String {
    let name_width = report
        .cases
        .iter()
        .map(|case| display_name(&case.aggregate).chars().count() + 2)
        .max()
        .unwrap_or(MIN_NAME_WIDTH)
        .max(MIN_NAME_WIDTH);
    let mut lines = vec![format!(
        "{:<7}{:<name_width$}{:<9}{:<11}{:<7}{:<13}{:<13}{:<8}{}",
        "RESULT",
        "CASE",
        "RATE",
        "THRESHOLD",
        "WRONG",
        "UNPARSEABLE",
        "UNAVAILABLE",
        "FAILED",
        "LATENCY",
    )];
    for case in &report.cases {
        lines.push(render_case(&case.aggregate, name_width));
    }
    lines.push(String::new());
    lines.push(render_footer(report));
    lines.join("\n")
}

/// A canary is marked in the table: its threshold of 1.0 is not a typo.
fn display_name(aggregate: &CaseAggregate) -> String {
    if aggregate.canary {
        format!("{} (canary)", aggregate.name)
    } else {
        aggregate.name.clone()
    }
}

fn render_case(aggregate: &CaseAggregate, name_width: usize) -> String {
    let latency = aggregate.mean_latency.map_or_else(
        || "-".to_owned(),
        |latency| format!("{} ms", latency.as_millis()),
    );
    format!(
        "{:<7}{:<name_width$}{:<9}{:<11}{:<7}{:<13}{:<13}{:<8}{}",
        if aggregate.met_threshold {
            "pass"
        } else {
            "FAIL"
        },
        display_name(aggregate),
        format!("{}/{}", aggregate.passes, aggregate.repeats),
        format!("{:.2}", aggregate.threshold),
        aggregate.wrong,
        aggregate.unparseable,
        aggregate.unavailable,
        aggregate.failed,
        latency,
    )
}

fn render_footer(report: &Report) -> String {
    let total = report.cases.len();
    let passed = report
        .cases
        .iter()
        .filter(|case| case.aggregate.met_threshold)
        .count();
    format!(
        "{total} cases, {passed} at or above threshold, {} below",
        total - passed
    )
}

#[cfg(test)]
#[path = "tests/report.rs"]
mod tests;
