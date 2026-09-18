//! Per-invocation directory helper: `<root>/<invocation-id>/report.json`.
//!
//! One directory per invocation, holding the report. The invocation id is a
//! UUID v7, so a listing of a root sorts by start time. Where the root
//! itself lives is the consumer's choice; this type only owns what happens
//! under it.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use uuid::Uuid;

use crate::report::{CaseReport, Report};

/// The report file inside an invocation directory.
pub const REPORT_FILE: &str = "report.json";

/// Failure to create or write an invocation's directory.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InvocationDirError {
    /// The invocation directory could not be created.
    #[error("failed to create the invocation directory at {path}")]
    Create {
        /// The directory that could not be created.
        path: PathBuf,
        /// The underlying cause.
        #[source]
        source: std::io::Error,
    },
    /// The report could not be serialized to JSON.
    #[error("failed to serialize the eval report")]
    Serialize(#[source] serde_json::Error),
    /// The report could not be written to disk.
    #[error("failed to write the eval report to {path}")]
    Write {
        /// Where the write was attempted.
        path: PathBuf,
        /// The underlying cause.
        #[source]
        source: std::io::Error,
    },
}

/// One invocation's directory and its stable identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationDir {
    id: Uuid,
    path: PathBuf,
}

impl InvocationDir {
    /// Create a fresh invocation directory under `root`.
    ///
    /// # Errors
    /// Returns [`InvocationDirError::Create`] when the directory cannot be
    /// created — never mid-suite, only before or after a run.
    pub fn create(root: &Path) -> Result<Self, InvocationDirError> {
        let id = Uuid::now_v7();
        let path = root.join(id.to_string());
        std::fs::create_dir_all(&path).map_err(|source| InvocationDirError::Create {
            path: path.clone(),
            source,
        })?;
        Ok(Self { id, path })
    }

    /// This invocation's UUID v7 identifier.
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// The invocation directory itself.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the JSON report lives.
    #[must_use]
    pub fn report_path(&self) -> PathBuf {
        self.path.join(REPORT_FILE)
    }

    /// Assemble a [`Report`] for this invocation, stamped with this
    /// directory's id and the current time.
    ///
    /// The recommended way to build a report for a consumer that pairs one
    /// with an `InvocationDir`: the report's `invocation_id` then matches
    /// the directory `write_report` saves it under, so the two never drift
    /// apart.
    #[must_use]
    pub fn report(&self, settings: serde_json::Value, cases: Vec<CaseReport>) -> Report {
        Report::new(self.id, Timestamp::now(), settings, cases)
    }

    /// Write `report` and hand back its path.
    ///
    /// # Errors
    /// Returns [`InvocationDirError::Serialize`] when the report cannot be
    /// turned into JSON, or [`InvocationDirError::Write`] when it cannot be
    /// written to disk.
    pub fn write_report(&self, report: &Report) -> Result<PathBuf, InvocationDirError> {
        let path = self.report_path();
        let json = serde_json::to_string_pretty(report).map_err(InvocationDirError::Serialize)?;
        std::fs::write(&path, format!("{json}\n")).map_err(|source| InvocationDirError::Write {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }
}

#[cfg(test)]
#[path = "tests/dir.rs"]
mod tests;
