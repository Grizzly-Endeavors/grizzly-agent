use jiff::Timestamp;
use uuid::Uuid;

use crate::report::Report;

use super::*;

#[test]
fn create_makes_a_directory_named_after_its_id() {
    let root = tempfile_dir();
    let dir =
        InvocationDir::create(root.path()).expect("creating the invocation directory must succeed");

    assert!(
        dir.path().is_dir(),
        "the invocation directory must exist on disk"
    );
    assert_eq!(
        dir.path(),
        root.path().join(dir.id().to_string()),
        "the directory must be named after the invocation's own id"
    );
}

#[test]
fn write_report_writes_valid_json_at_report_path() {
    let root = tempfile_dir();
    let dir =
        InvocationDir::create(root.path()).expect("creating the invocation directory must succeed");
    let report = Report::new(
        Uuid::now_v7(),
        Timestamp::now(),
        serde_json::json!({}),
        Vec::new(),
    );

    let written_path = dir
        .write_report(&report)
        .expect("writing the report must succeed");
    assert_eq!(
        written_path,
        dir.report_path(),
        "write_report must return the same path report_path predicts"
    );

    let contents =
        std::fs::read_to_string(&written_path).expect("the report file must be readable");
    let restored: Report =
        serde_json::from_str(&contents).expect("the written file must be valid JSON");
    assert_eq!(
        restored, report,
        "the written report must match what was passed in"
    );
}

/// A directory this test owns for its whole lifetime, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn tempfile_dir() -> TempDir {
    let path = std::env::temp_dir().join(format!("grizzly-agent-eval-test-{}", Uuid::now_v7()));
    TempDir(path)
}
