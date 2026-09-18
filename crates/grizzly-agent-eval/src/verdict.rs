//! What one repeat produced: whether it passed, and why.

use std::time::Duration;

use grizzly_agent_core::Usage;
use serde::{Deserialize, Serialize};

/// Why a repeat landed where it did.
///
/// [`VerdictCategory::Unparseable`] and [`VerdictCategory::Unavailable`] are
/// never merged: a readable answer the parser could not read is a finding
/// about the model, and no answer at all — a timeout, a transport failure, a
/// rejected request — is a finding about the endpoint. Collapsing the two
/// would score a dead endpoint as a model regression, or the reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictCategory {
    /// Every expectation held.
    Pass,
    /// A parseable answer that was not the expected one.
    Wrong,
    /// The model answered and the parser could not read it.
    Unparseable,
    /// No answer came back at all: a timeout, a transport failure, or a
    /// rejected request.
    Unavailable,
    /// Harness or infrastructure trouble: nothing was measured at all.
    Failed,
}

/// One repeat's verdict: whether it passed, why, and what it cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// This verdict's category.
    pub category: VerdictCategory,
    /// A human-readable explanation of the category.
    pub reason: String,
    /// How long the repeat took, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<Duration>,
    /// Token usage the repeat reported, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl Verdict {
    /// A passing verdict.
    #[must_use]
    pub fn pass(reason: impl Into<String>) -> Self {
        Self::new(VerdictCategory::Pass, reason)
    }

    /// A verdict for a readable answer that was not the expected one.
    #[must_use]
    pub fn wrong(reason: impl Into<String>) -> Self {
        Self::new(VerdictCategory::Wrong, reason)
    }

    /// A verdict for a reply the parser could not read.
    #[must_use]
    pub fn unparseable(reason: impl Into<String>) -> Self {
        Self::new(VerdictCategory::Unparseable, reason)
    }

    /// A verdict for no reply at all.
    #[must_use]
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::new(VerdictCategory::Unavailable, reason)
    }

    /// A verdict for harness or infrastructure trouble that measured nothing.
    #[must_use]
    pub fn failed(reason: impl Into<String>) -> Self {
        Self::new(VerdictCategory::Failed, reason)
    }

    fn new(category: VerdictCategory, reason: impl Into<String>) -> Self {
        Self {
            category,
            reason: reason.into(),
            latency: None,
            usage: None,
        }
    }

    /// This verdict, carrying `latency`.
    #[must_use]
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = Some(latency);
        self
    }

    /// This verdict, carrying `usage`.
    #[must_use]
    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Whether this verdict counts as a pass.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.category == VerdictCategory::Pass
    }
}

#[cfg(test)]
#[path = "tests/verdict.rs"]
mod tests;
