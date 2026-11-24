# AGENTS.md | grizzly-agent

Project-specific guidance for agents working in this repository. Universal rules live in `~/Projects/AGENTS.md`.

## Build & Quality Gates

Every command runs across the whole Cargo workspace:

```sh
cargo test --workspace --quiet                           # default features
cargo test --workspace --all-features --quiet             # includes prompts codegen/verify
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings     # catches feature-gating bugs
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
```

`just ci-local` runs the full gate plus `just provider-matrix` (no provider, each provider alone, and both). Pre-commit hooks (`./.githooks/install.sh` or `just hooks`) run fmt → clippy → tests → `cargo deny` and reject staged `dbg!()` or `#[allow(]`.

## Library Rules

This workspace is consumed as a shared dependency across repos:

- **Lint table:** Defined once in root `Cargo.toml` and inherited via `lints.workspace = true`. Includes `missing_docs`, `unreachable_pub`, `missing_errors_doc`, `missing_panics_doc`, `error_impl_error`, and `exit = "deny"`.
- **Test-module lint trap:** `clippy.toml` exempts `unwrap_used`, `expect_used`, `panic`, and `dbg_macro` in tests. **Do not add `#[expect(clippy::unwrap_used, ...)]` to test files** — it becomes an unfulfilled-expectation error under `-D warnings`.
- **Integration tests (`tests/*.rs`):** `tests_outside_test_module` is not test-exempt; start every integration test file with `#![expect(clippy::tests_outside_test_module, reason = "integration tests live at crate root by cargo convention")]`.
- **Public errors:** Public APIs return `thiserror` enums preserving `#[source]`, never `anyhow::Error`. `anyhow` is fine inside private helpers only.
- **Async & tracing:** Expose `async fn` and emit `tracing` events; never start a Tokio runtime or install a `tracing-subscriber` (`tracing-subscriber` is dev-only). No blocking calls inside async functions.
- **No `std::process::exit`:** Return `Result` and let the consumer decide.

## Module & Test Layout

- **Pure core vs shell:** `grizzly-agent-core` has no HTTP/process/filesystem I/O — external I/O enters through traits implemented by consumers or `grizzly-agent-providers`.
- **Sibling unit tests:** Keep unit tests out of impl files; put them in `tests/<module>.rs` and load via `#[cfg(test)] #[path = "tests/<module>.rs"] mod tests;`.
- **Env vars in tests:** Never call `unsafe` `std::env::set_var` / `remove_var`. Every env-reading API exposes a public `from_process_env()` alongside a `from_env(EnvLookup<'_>)` sibling so tests and consumers can inject credentials without touching process env.
