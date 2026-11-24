use super::*;

const GREETING: &str = "---
id: Greeting
type: prompt
annotations:
  sent_when: at conversation start
  used_by:
    - file: discord.rs
      function: greet
  reasoning:
    - opens every conversation
---
Hello there.";

const FAREWELL: &str = "---
id: Farewell
type: prompt
annotations:
  sent_when: at conversation end
  used_by:
    - file: present.rs
      function: bye
  reasoning:
    - closes the conversation
---
Goodbye.";

fn write_tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, content) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }
    dir
}

fn run(prompts: &[(&str, &str)], src: &[(&str, &str)]) -> Result<(), VerifyReport> {
    let prompts_dir = write_tree(prompts);
    let src_dir = write_tree(src);
    verify_annotations(prompts_dir.path(), src_dir.path())
}

#[test]
fn passes_when_used_by_file_names_id_and_function() {
    let result = run(
        &[("Greeting.md", GREETING)],
        &[("discord.rs", "fn greet() -> String { Greeting::render() }")],
    );
    assert!(
        result.is_ok(),
        "unexpected failures: {}",
        result.unwrap_err()
    );
}

#[test]
fn resolves_used_by_paths_in_subdirectories() {
    let greeting = GREETING.replace("file: discord.rs", "file: gary/discord.rs");
    let result = run(
        &[("Greeting.md", greeting.as_str())],
        &[("gary/discord.rs", "fn greet() { Greeting::render(); }")],
    );
    assert!(
        result.is_ok(),
        "unexpected failures: {}",
        result.unwrap_err()
    );
}

#[test]
fn flags_missing_source_file() {
    let report = run(
        &[("Greeting.md", GREETING)],
        &[("elsewhere.rs", "// Greeting is referenced here")],
    )
    .unwrap_err();
    let failures = report.failures();
    assert_eq!(failures.len(), 1, "expected one failure, got: {report}");
    assert!(
        matches!(
            failures.first(),
            Some(VerifyError::MissingSourceFile { .. })
        ),
        "got: {report}"
    );
}

#[test]
fn flags_used_by_file_missing_the_id() {
    let report = run(
        &[("Greeting.md", GREETING)],
        &[
            ("discord.rs", "fn greet() {}"),
            ("elsewhere.rs", "// Greeting lives here"),
        ],
    )
    .unwrap_err();
    let failures = report.failures();
    assert_eq!(failures.len(), 1, "expected one failure, got: {report}");
    assert!(
        matches!(failures.first(), Some(VerifyError::IdNotInFile { .. })),
        "got: {report}"
    );
}

#[test]
fn flags_used_by_file_missing_the_function() {
    let report = run(
        &[("Greeting.md", GREETING)],
        &[("discord.rs", "fn other() { Greeting::render(); }")],
    )
    .unwrap_err();
    let failures = report.failures();
    assert_eq!(failures.len(), 1, "expected one failure, got: {report}");
    assert!(
        matches!(
            failures.first(),
            Some(VerifyError::FunctionNotInFile { .. })
        ),
        "got: {report}"
    );
}

#[test]
fn flags_orphaned_prompt() {
    let report = run(
        &[("Greeting.md", GREETING)],
        &[("discord.rs", "fn greet() {}")],
    )
    .unwrap_err();
    assert!(
        report
            .failures()
            .iter()
            .any(|e| matches!(e, VerifyError::OrphanedPrompt { .. })),
        "expected an orphaned-prompt failure, got: {report}"
    );
}

#[test]
fn aggregates_failures_across_prompts() {
    let report = run(
        &[("Greeting.md", GREETING), ("Farewell.md", FAREWELL)],
        &[
            ("refs.rs", "// Greeting Farewell"),
            ("present.rs", "fn bye() {}"),
        ],
    )
    .unwrap_err();
    let failures = report.failures();
    assert_eq!(failures.len(), 2, "expected two failures, got: {report}");
    assert!(
        failures
            .iter()
            .any(|e| matches!(e, VerifyError::MissingSourceFile { .. })),
        "missing the MissingSourceFile failure: {report}"
    );
    assert!(
        failures
            .iter()
            .any(|e| matches!(e, VerifyError::IdNotInFile { .. })),
        "missing the IdNotInFile failure: {report}"
    );
}

#[test]
fn propagates_load_failure() {
    let broken = "---
id: Broken
type: prompt
annotations:
  sent_when: whenever
  used_by:
    - file: x.rs
      function: f
---
Body.";
    let report = run(&[("Broken.md", broken)], &[]).unwrap_err();
    let failures = report.failures();
    assert_eq!(failures.len(), 1, "expected one failure, got: {report}");
    assert!(
        matches!(failures.first(), Some(VerifyError::Load(_))),
        "got: {report}"
    );
}
