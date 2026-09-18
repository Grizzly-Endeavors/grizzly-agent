use std::path::PathBuf;

use super::super::DEFAULT_CRATE_PATH;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid")
}

/// Text-only golden for the default crate path (`grizzly_agent`). Never
/// compiled here — this crate has no `grizzly_agent` dependency to compile it
/// against; `tests/golden.rs` compiles the `grizzly_agent_core`-targeted
/// sibling golden generated from the same fixture tree.
fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/codegen/emit/tests/fixtures/expected_prompts.rs")
}

#[test]
#[ignore = "run to regenerate the golden: cargo test -p grizzly-agent-prompts --features codegen regenerate_golden -- --ignored"]
fn regenerate_golden() {
    let source = super::generate(&fixtures_dir(), DEFAULT_CRATE_PATH).unwrap();
    std::fs::write(golden_path(), source).unwrap();
}

/// The checked-in golden must match what the emitter produces today. If this
/// fails, the emitter changed — rerun `regenerate_golden` and review the diff
/// (and regenerate `tests/fixtures/expected_prompts.rs` at the crate root the
/// same way, targeting `grizzly_agent_core`).
#[test]
fn golden_is_current() {
    let generated = super::generate(&fixtures_dir(), DEFAULT_CRATE_PATH).unwrap();
    let golden = std::fs::read_to_string(golden_path()).unwrap();
    assert_eq!(generated, golden, "golden is stale; regenerate it");
}

#[test]
fn emit_to_writes_prompts_rs() {
    let out = tempfile::tempdir().unwrap();
    super::emit_to(&fixtures_dir(), out.path(), DEFAULT_CRATE_PATH).unwrap();
    let written = std::fs::read_to_string(out.path().join("prompts.rs")).unwrap();
    assert_eq!(
        written,
        super::generate(&fixtures_dir(), DEFAULT_CRATE_PATH).unwrap()
    );
}

#[test]
fn generated_source_wraps_tool_spec_strings_as_cow_borrowed() {
    let generated = super::generate(&fixtures_dir(), DEFAULT_CRATE_PATH).unwrap();
    assert!(
        generated.contains("name: ::std::borrow::Cow::Borrowed(Self::NAME),"),
        "spec() must wrap the name as a borrowed Cow: {generated}"
    );
    assert!(
        generated.contains("description: ::std::borrow::Cow::Borrowed("),
        "spec() must wrap the description as a borrowed Cow: {generated}"
    );
}

#[test]
fn generated_source_implements_tool_definition_for_every_tool() {
    let generated = super::generate(&fixtures_dir(), DEFAULT_CRATE_PATH).unwrap();
    for (tool, params) in [
        ("EditFile", "EditFileParams"),
        ("Ping", "grizzly_agent::NoParams"),
        ("StartServer", "NameParams"),
    ] {
        let impl_line = format!("impl grizzly_agent::ToolDefinition for {tool} {{");
        assert!(
            generated.contains(&impl_line),
            "{tool} must implement ToolDefinition: {generated}"
        );
        let params_line = format!("type Params = {params};");
        assert!(
            generated.contains(&params_line),
            "{tool}'s ToolDefinition must bind Params = {params}: {generated}"
        );
    }
}

#[test]
fn generated_source_targets_a_non_default_crate_path() {
    let generated = super::generate(&fixtures_dir(), "grizzly_agent_core").unwrap();
    assert!(
        generated.contains("fn spec() -> grizzly_agent_core::ToolSpec {"),
        "spec() must target the configured crate path: {generated}"
    );
    assert!(
        !generated.contains("grizzly_agent::"),
        "no reference to the default crate path should remain: {generated}"
    );
}
