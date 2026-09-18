# ADR-0012 — Generated tools expose `spec()` only through `ToolDefinition`

**Status:** Accepted (2026-09-18)

## Context

`grizzly-agent-prompts` codegen emits, per tool, a unit struct with an inherent `NAME` constant, and previously also an inherent `pub fn spec() -> ToolSpec`, plus an `impl ToolDefinition for Tool { type Params = ...; fn spec() -> ToolSpec { Self::spec() } }` that delegated to it. The inherent form was kept alongside the trait impl so a consumer's existing `Tool::spec()` call sites — written against the pre-`ToolDefinition` generated code — kept compiling unchanged.

Having both an inherent `spec()` and a trait `spec()` on the same type trips `clippy::same_name_method`, which this workspace and its consumers deny. The generated code carried a scoped `#[expect(clippy::same_name_method, reason = "...")]` on the inherent method to satisfy that. But codegen emits this suppression into every consumer's build output, not just this workspace's own lint config — a consumer that does not enable `clippy::same_name_method` (a restriction lint, off by default) would very likely see `#[expect]` itself flagged as an unfulfilled expectation once `-D warnings` runs, since the lint it names never fires without the consumer opting in. Shipping a suppression that only sometimes has anything to suppress is a defect in the generated code, not a one-off.

## Decision

**Generated tools expose a single `spec()`, reachable only through the `ToolDefinition` impl.** The inherent `spec()` and its `#[expect(clippy::same_name_method, ...)]` are gone; the `ToolSpec` construction body that used to live in the inherent method now lives directly in `ToolDefinition::spec()`. `NAME` stays an inherent `&'static str` constant, unchanged. Callers write `Tool::spec()` with `ToolDefinition` in scope, or `<Tool as ToolDefinition>::spec()` without it.

## Consequences

- Consumers of generated tools call `Tool::spec()` through the trait: every call site needs `use grizzly_agent_core::ToolDefinition;` (or the facade's re-export) in scope. `gameservers` adds one import in four files.
- No `#[expect(clippy::same_name_method, ...)]` ships in generated code, so consumers never see it — nothing to go unfulfilled under a lint config that doesn't enable the restriction lint it names.
- `NAME` is unaffected: it was never part of this collision and needed no change.
