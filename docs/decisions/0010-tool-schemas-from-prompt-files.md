# ADR-0010 — A tool's schema and handler come from a prompt file, not a schemars struct

**Status:** Accepted (2026-09-18)
**Supersedes:** [ADR-0006](0006-tool-schemas-from-one-struct.md)

## Context

ADR-0006 closed the schema/handler drift the primitive-index survey found in three independent projects by deriving a tool's JSON Schema and its parsed argument type from one `schemars`-annotated struct. That closes the drift, but it puts a tool's model-facing description in a Rust attribute rather than in prose, and `schemars`-generated schema output needs attribute nudging to read well to a model — real, repeated work, done in a place other than where the rest of a tool's model-facing text lives.

`grizzly-gameservers`'s `prompt-lib` already solves this differently and further along: it compiles a tool's description and parameter schema from the same reviewed Markdown prompt file as the rest of the model-facing text that ships with it, generating both the wire schema and a params type from one file at build time. ADR-0006 rejected build-time generation for this crate specifically because it cannot describe a tool constructed at runtime — but `prompt-lib`'s shape and a runtime-constructible tool model are not actually in tension, once tool definition is split from tool construction.

## Decision

**`prompt-lib`'s tool model, promoted to this crate's single definition of a tool.** `ToolSpec` is the wire advertisement (name, description, and a JSON Schema value); `ToolDefinition` is a trait binding a tool type to its spec and its associated `Params` type. Codegen (now `grizzly-agent-prompts`' `codegen` feature) emits a `ToolDefinition` impl for every generated tool, so the schema advertised and the type its arguments parse into come from the same prompt file and cannot drift — the same property ADR-0006 wanted, produced by generation from prose instead of derivation from a Rust struct. A hand-written runtime tool implements `ToolHandler` directly, taking raw JSON arguments; core's typed adapter turns a `ToolDefinition` plus an async function into a `ToolHandler`, parsing arguments and converting a parse failure into `ToolFailure::InvalidArguments` phrased for the model. `schemars` is dropped from the dependency tree entirely.

### Rejected: keep `schemars`, generate `ToolDefinition` impls around it

Would preserve today's schema generation while adding the drift-closing binding. Rejected because it keeps a tool's model-facing description split across a Rust attribute and a prompt file for tools that have one, solving a problem the prompt-file model does not have.

### Rejected: a `tool!` declarative macro (reconsidered from ADR-0006)

Still rejected for the reason ADR-0006 gave it: macro-generated code produces worse compiler diagnostics than trait impls, and it reintroduces hand-writing schemas for any tool not covered by codegen.

## Consequences

- A tool's schema and handler type are guaranteed to agree only for **generated** tools — the ones with a prompt file. A hand-written runtime tool (`ToolHandler` implemented directly) has no compile-time link between its advertised `ToolSpec` and what it parses from the arguments; that tool's author is responsible for keeping them in agreement, the same trade `prompt-lib` always made for hand-written dispatch.
- Tool definition and tool construction are split: a generated `ToolDefinition` describes a tool's shape at build time, while `ToolHandler` is what the turn loop actually calls at runtime. A tool built at runtime (skills, an MCP-backed wrapper, anything from config) is a `ToolHandler` with no `ToolDefinition` at all, which is what ADR-0006's build-time rejection was really asking for.
- `ToolSpec`'s name and description are `Cow<'static, str>`: generated tools use borrowed statics at zero cost, and runtime-constructed tools own their strings.
- Consumers depending on `grizzly-agent-prompts` for codegen take a build-edge dependency and a second dependency line beyond the facade, in exchange for not compiling core into every build script (see the workspace design's prompts-outside-the-facade reasoning).
