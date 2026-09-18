//! Case metadata every eval case carries, independent of what it measures.

use serde::Deserialize;

/// Pass rate a case must reach when it sets none of its own.
pub const DEFAULT_MIN_PASS_RATE: f64 = 2.0 / 3.0;

/// Repeats to run for a case when neither the case nor the caller overrides
/// it.
pub const DEFAULT_REPEATS: u32 = 3;

/// The fields every eval case carries, regardless of what it measures.
///
/// Deserializable on its own, so a consumer embeds it in its own case
/// document with `#[serde(flatten)]` and gets `name`, `repeats`,
/// `min_pass_rate`, and `canary` for free.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CaseMeta {
    /// The case's name, unique within whatever corpus holds it.
    pub name: String,
    /// Repeats this case runs, overriding the run's default when set.
    #[serde(default)]
    pub repeats: Option<u32>,
    /// The pass rate this case must reach, overriding
    /// [`DEFAULT_MIN_PASS_RATE`] when set. Ignored on a canary, whose
    /// threshold is always 1.0.
    #[serde(default)]
    pub min_pass_rate: Option<f64>,
    /// A canary case's threshold is pinned at 1.0 regardless of
    /// `min_pass_rate`: any miss fails it.
    #[serde(default)]
    pub canary: bool,
}

impl CaseMeta {
    /// Metadata for a case built at runtime rather than deserialized from a
    /// file, with no repeats or pass-rate override and not a canary.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            repeats: None,
            min_pass_rate: None,
            canary: false,
        }
    }

    /// Repeats to run: this case's own count wins over `suite_default`.
    #[must_use]
    pub fn effective_repeats(&self, suite_default: u32) -> u32 {
        self.repeats.unwrap_or(suite_default)
    }

    /// The pass rate this case must reach. A canary's threshold is always
    /// 1.0, regardless of `min_pass_rate`.
    #[must_use]
    pub fn threshold(&self) -> f64 {
        if self.canary {
            return 1.0;
        }
        self.min_pass_rate.unwrap_or(DEFAULT_MIN_PASS_RATE)
    }
}

#[cfg(test)]
#[path = "tests/case.rs"]
mod tests;
