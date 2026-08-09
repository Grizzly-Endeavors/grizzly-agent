# ADR-0006 — A tool's schema and its handler come from one struct

**Status:** Accepted (2026-08-09)

## Context

The most-repeated friction in `docs/design/primitive-index.md` is that a tool's declared schema and the code implementing it live in separate places and drift. Three projects hit it independently:

- `grizzly-gameservers` built `prompt-lib`, which compiles prompt files into typed Rust and generates each tool's JSON Schema and params struct — then stops short of dispatch, leaving roughly thirty hand-written match arms.
- `poe2/mcp` declares each tool's schema in `list_tools()` and its handler in a parallel thirty-branch `if/elif` about seven hundred lines away, with nothing checking they agree.
- `job-finder` splits dispatch three ways — an MCP name-set check, one hardcoded special case, and a handler dict — so adding a tool means remembering three places.

Every one of these works. Each is also one careless edit away from advertising a schema whose arguments the handler cannot parse, which surfaces as the model being confused rather than as a compile error.

## Decision

**A tool is defined by one params struct deriving `JsonSchema` and `Deserialize`.** The wire schema is generated from it, and the handler receives it already parsed and typed. They cannot disagree, because there is only one declaration.

`schemars` 1.2.2 does the generation. Verified current and actively maintained at the time of pinning.

### Rejected: a `tool!` declarative macro

Registers name, schema, and handler together with no `schemars` dependency and full control over the emitted JSON. Rejected on two counts. Hand-writing schemas is the tedium three projects already independently found tedious, so this solves drift by reintroducing the work that caused it. And macro-generated code produces notoriously worse compiler diagnostics than trait impls — a mistyped field in a tool definition should point at the field, not at a macro expansion.

### Rejected: a plain trait with a hand-written `definition()`

This is `residuum`'s existing shape (`src/tools/mod.rs:212`), and it is the honest minimum: `definition()` and `execute()` on one impl, so at least they are adjacent. Rejected because adjacency is not agreement. It narrows the drift window from seven hundred lines to twenty without closing it, and twenty lines is still far enough to add a required field to a schema and forget the parse.

### Rejected: generating schemas at build time, as `prompt-lib` does

Genuinely good — it also catches orphaned and stale prompts by cross-referencing declared call sites. Rejected for this crate because build-time generation cannot describe a tool constructed at runtime, and a general toolkit should not forbid that. `prompt-lib`'s verify pass remains the better answer for prompt staleness specifically, which is a different problem.

## Consequences

- `schemars` is a non-optional dependency, since tool definition is core to the crate rather than an optional feature.
- Generated schemas sometimes need attribute nudging (`#[schemars(description = "...")]`) to read well to a model. This is real work, but it is work done once per tool in the same place as everything else about that tool.
- A tool whose arguments genuinely cannot be typed — free-form passthrough — can still take `serde_json::Value` as its params struct. The seam does not forbid it; it just makes the typed case the easy one.
