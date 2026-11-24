# AGENTS.md | grizzly-agent

Project-specific guidance for agents working in this repository. Universal rules live in `~/Projects/AGENTS.md`.

## Build & Quality Gates

This repository is a Cargo workspace; every command below runs across the whole workspace, not a single member.

```sh
cargo doc --workspace --no-deps --all-features           # read the public API you just changed
cargo test --workspace --quiet                           # always --quiet; never plain cargo test
cargo test --workspace --all-features --quiet             # also with all features — some logic (e.g. prompts' codegen/verify) only compiles then
cargo fmt --all                                          # format
cargo fmt --all -- --check                               # verify formatting
cargo clippy --workspace --all-targets -- -D warnings                # default features — catches gating bugs all-features hides
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check                                         # advisories, licenses, sources
```

The `justfile` wraps these: `just test`, `just lint`, `just deny`, `just ci-local` (the full gate, which also runs `just provider-matrix` — building the facade with no provider feature, each provider alone, and both). Every recipe is also a plain cargo command — `just` is convenience, not required.

**Pre-commit hooks** run fmt (auto-applied and re-staged) → clippy → tests → `cargo deny` if installed, then reject staged `dbg!()` and `#[allow(` anywhere in the workspace. Install once with `./.githooks/install.sh` or `just hooks`. **Bypassing with `--no-verify` is forbidden.** If a hook fails, fix the underlying issue.

## Workspace Layout

Members live under `crates/`: `crates/grizzly-agent` (the facade, re-exporting the runtime crates behind features), `crates/grizzly-agent-core` (conversation types, the error taxonomy, the tool model, model calls, and the turn loop and `Agent`; no HTTP), `crates/grizzly-agent-providers` (concrete provider clients — Anthropic and OpenAI-compatible — each behind its own feature; depends on core), `crates/grizzly-agent-prompts` (prompt-file frontmatter parsing, codegen, and call-site verification; does not depend on core), `crates/grizzly-agent-eval` (`ResponseEval` and `AgentEval` over the shared scoring/report core they both feed; depends on core, not on providers), and `crates/grizzly-agent-skills` (Agent Skills format, index, and activation, per the open Agent Skills specification; depends on core and on prompts' default face). The lint table, toolchain pin, and deny policy live once at the workspace root; every member's `Cargo.toml` inherits it unchanged with `lints.workspace = true` and pulls shared dependency versions from `[workspace.dependencies]`. A member's own `[dependencies]` lists only what that crate actually uses — the facade takes on a new runtime dependency only when the crate providing it exists and one of the facade's features enables it.

## Lint Configuration

Clippy is configured with strict denies — not warnings. The lint block in `Cargo.toml` is intentional and not to be relaxed without explicit approval.

### What's denied and why

- **`unwrap_used`, `expect_used`, `panic`, `get_unwrap`**: no panics on untrusted input or error paths. Use `?`, `anyhow::Context`, or explicit handling.
- **`todo`, `unimplemented`, `dbg_macro`**: no incomplete or debug code ships.
- **`exit`**: nothing here may call `std::process::exit`. This crate is a dependency — terminating the host process is never its call to make. Return a `Result` and let the consumer decide.
- **`indexing_slicing`, `string_slice`**: use `.get()` / slice-returning methods that produce `Option`.
- **`map_err_ignore`, `let_underscore_must_use`**: no silent error swallowing.
- **`error_impl_error`**: don't name a domain error type `Error`. It collides with every other crate's and reads as "the" error type.
- **`missing_errors_doc`, `missing_panics_doc`, `must_use_candidate`**: public API must document its failure modes and must-use returns.
- **`missing_docs`, `unreachable_pub`** (rustc, not clippy): this crate is consumed as a dependency, so its public surface *is* the product. An undocumented public item is a defect here in a way it isn't in a binary, and `unreachable_pub` mechanically catches a `pub` that no external path can reach — the usual sign the visibility was cargo-culted rather than chosen.
- **`allow_attributes`, `allow_attributes_without_reason`**: every suppression uses `#[expect(..., reason = "...")]`, never `#[allow(...)]`. `#[expect]` warns when the suppression goes stale; `#[allow]` sits there forever.
- **`tests_outside_test_module`**: tests live in `#[cfg(test)]` modules. Integration tests in `tests/` need the file-level escape below.
- **`too_many_lines`**: keep functions short.
- **`wildcard_enum_match_arm`, `shadow_unrelated`**: no lazy match catch-alls, no surprise variable shadowing.
- **`rc_buffer`, `rc_mutex`**: antipatterns.
- **`clone_on_ref_ptr`, `format_push_string`, `redundant_type_annotations`**: code quality.

### Relaxed

- `pedantic` is at `warn` with `priority = -1`: pedantic baseline, but `deny` would break builds on toolchain upgrades that add new lints. Note that `-D warnings` in the lint command still makes them blocking — the level exists so a toolchain bump is a fixable failure, not a broken build.
- `module_name_repetitions = "allow"`: pervasive pattern in this codebase style.

### Unsafe

`unsafe_code = "deny"` (not `forbid`) — `deny` allows a scoped `#[expect(unsafe_code, reason = "FFI boundary")]` at the item level when genuinely needed. `forbid` can't be overridden item-level, which turns one legitimate FFI shim into a reason to weaken the whole config.

### Test modules

Test code may freely use `.unwrap()`, `.expect()`, `panic!`, and `dbg!` — the root `clippy.toml` exempts all four in tests.

**Do not add `#[expect(clippy::unwrap_used, ...)]` to test files.** The lint is already exempt, so the expectation never fires and becomes an unfulfilled-expectation hard error under `-D warnings`. This is the single most common way to break this config.

`tests_outside_test_module` is *not* test-exempt — see the integration-test boilerplate below.

## Async

- **Tokio is the assumed runtime, but this crate never starts one.** A library that calls `Runtime::new` or `block_on` inside an async caller deadlocks it. Expose `async fn` and let the consumer's `#[tokio::main]` drive them. `tokio::spawn` and `tokio::select!` are fine *inside* an async fn — they require a running runtime, they don't create one.
- **Async-first**: use async for I/O; fall back to sync only for CPU-bound or genuinely trivial operations.
- **No blocking calls inside async functions.** Use `tokio::fs`, `tokio::process`, etc. If a blocking call is unavoidable, wrap it in `spawn_blocking`.
- **Shared state**: wrap in `Arc<T>` with thread-safe interior mutability (`DashMap`, `Mutex`/`RwLock` when contention is low, channels for message passing).
- **Graceful shutdown**: listen for `SIGINT`/`SIGTERM` via `tokio::signal`; let the `tokio::select!` in the main loop fall out on signal and drain in-flight work.
- **Emit tracing events; never install a subscriber.** Choosing the subscriber is the consumer's call, which is why `tracing-subscriber` is a dev-dependency here and not a real one.

## Error Handling

- **The public API returns `thiserror` enums, never `anyhow::Error`.** This inverts the usual default, and it is the most important error rule in this repo. A consumer needs to distinguish "rate limited, back off and retry" from "malformed request, fixing the retry won't help" — an opaque `anyhow::Error` forces them to string-match the message to find out, which breaks the moment the wording changes. Typed errors are the difference between a library a caller can build a retry policy on and one they can only log.
- **`anyhow` is fine internally.** Inside a private helper, chain context with `.context("failed to parse tool schema")` freely. It just may not reach the public boundary — convert to a typed variant on the way out.
- **Preserve the cause.** Typed variants carry `#[source]` so the full chain survives conversion. Widening to a typed error must not flatten what went wrong into a string.
- **No `.unwrap()` / `.expect()` outside of tests.** If you genuinely know a value is present, use `.expect("reason — invariant explanation")` inside a scoped `#[expect(clippy::expect_used, reason = "...")]` with a real reason.
- **No silent error swallowing.** `let _ = ...` on a `Result` is denied; so is `.map_err(|_| ...)`. Every error either propagates or gets logged with context before being handled.

## Module Layout

- **`mod.rs`** primarily contains declarations and curated `pub use` re-exports. Module-level coordination logic is fine when it belongs there; gratuitous plumbing is not.
- **Group related types in one file** (e.g. `Message`, `Role`, `ToolCall` together in `llm/types.rs`) rather than one-type-per-file.
- **Shell vs core split**: IO-bound and framework-bound code (HTTP handlers, event loops, GUI callbacks) lives in a thin *shell* layer that calls into *pure* modules where all the logic is. The shell is usually not unit-tested; the pure modules are. If you find yourself adding logic to a shell module, move it into a pure module first and let the shell call the validated result.
- **There is no shell in `grizzly-agent-core`.** It is all core — the shell belongs to the consumer, or to a provider crate that depends on core. What would be a shell layer elsewhere (HTTP transport, process spawning, filesystem access) appears in core as a *trait* the consumer implements, which is also what makes the logic testable without a network.
- **Boundaries at the domain's joints.** Modules should divide where the problem divides, so a change to one concern touches one place. A boundary drawn because a file got long produces a `utils.rs` grab-bag; a boundary drawn at a real seam produces modules you can reason about alone.

## Test Organization (Rust)

### Unit test file layout

Unit tests do **not** live at the bottom of the impl file. They live in a sibling `tests/` subdirectory, loaded via a `#[path]` module declaration at the bottom of the impl file. This keeps impl files short and prevents a three-line edit from dragging hundreds of lines of test noise into the context window.

- `src/foo.rs` → tests at `src/tests/foo.rs`
- `src/foo/bar.rs` → tests at `src/foo/tests/bar.rs`
- `src/foo/mod.rs` → tests at `src/foo/tests/foo.rs` (named after the module, not `mod.rs`)

Declared at the bottom of the impl file as:

```rust
#[cfg(test)]
#[path = "tests/foo.rs"]
mod tests;
```

The `#[path]` attribute makes the loaded file a **child** of the impl module, so `use super::*;` retains full access to private items — no visibility inflation.

The `#[cfg(test)]` guard matters: without it the file is compiled into consumers' builds.

### Integration test boilerplate

Every `tests/*.rs` file at crate root must start with:

```rust
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]
```

This is required because `clippy::tests_outside_test_module` is denied project-wide and fires on top-level `#[test]` functions — which are exactly what integration tests are. Unlike `unwrap_used`, it is not controllable from `clippy.toml`, so it still needs the per-file `#[expect]`.

### What gets tested

- **Pure modules** (parsing, validation, state transitions, path manipulation, error construction): every pure module ships with tests on the day it lands.
- **Shell modules** (HTTP handlers, framework integrations, event loops): usually not unit-tested. Exercised via integration tests or manual verification.

### Env var handling in tests

Rust 2024 made `std::env::set_var` / `remove_var` `unsafe`. **Do not use them in tests.** Instead, structure env-reading functions as a dual pair:

- A public zero-arg function that reads the real process env (e.g. `Settings::from_process_env()`).
- A public `_from_env` sibling taking an `EnvLookup<'_>` closure (e.g. `Settings::from_env(get)`).

The zero-arg function calls the sibling with `&|k| std::env::var_os(k)`. Tests construct closures with fixed keys.

This matters more here than in a binary: credential loading (`ANTHROPIC_API_KEY` and friends) is the main thing this crate reads from the environment, and a consumer must be able to bypass the process env entirely and supply a key from its own config or secret store. Every env read gets the closure sibling — no exceptions.

### Rationale

This layout is optimized for LLM agent workflows where the default "read the whole file" cost is multiplied by every edit. Sibling test files preserve Rust's native test ergonomics (private access, `cargo test` runs them automatically) while dramatically cutting read-time noise on mature modules.
