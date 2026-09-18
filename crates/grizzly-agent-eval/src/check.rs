//! The outcome of one post-run check an eval case runs after the work is
//! done — a hidden oracle the agent never saw, in `AgentEval`'s terms.

use serde::{Deserialize, Serialize};

/// One post-run check's outcome: a name, whether it passed, and a detail
/// string — for example a command's exit status and output tail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    /// What this check verifies, as it appears in the report.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Detail explaining the outcome. A consumer that needs richer check
    /// data keeps it here as text rather than growing this type.
    pub detail: String,
}

impl CheckResult {
    /// A check that passed.
    #[must_use]
    pub fn passed(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: true,
            detail: detail.into(),
        }
    }

    /// A check that failed.
    #[must_use]
    pub fn failed(name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            passed: false,
            detail: detail.into(),
        }
    }
}

#[cfg(test)]
#[path = "tests/check.rs"]
mod tests;
