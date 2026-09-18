# Architectural Decision Records

One file per non-obvious decision, named `NNNN-short-title.md`.

Each record covers: the context that forced a choice, the decision, the alternatives considered and why they lost, and the consequences accepted. An ADR should be readable in under 60 seconds.

Write one when a decision would make future-you ask "why on earth is it like this" — choosing one tool or approach over another, picking a topology or data model, deciding what to expose versus keep internal, or deciding to skip something. Don't write one for obvious choices.

## Index

| ADR | Title | Status |
| --- | --- | --- |
| [0001](0001-scope-boundary.md) | The admission test is two real consumers, not usefulness | Superseded by [0008](0008-admission-bar-and-workspace-shape.md) |
| [0002](0002-no-mcp.md) | MCP stays out, permanently | Accepted |
| [0003](0003-no-streaming.md) | No streaming until a consumer proves it needs it | Superseded by [0009](0009-streaming-is-the-provider-primitive.md) |
| [0004](0004-provider-reported-token-counts.md) | Token counts come from the provider | Accepted |
| [0005](0005-object-safe-provider-trait.md) | The provider trait is object-safe | Accepted |
| [0006](0006-tool-schemas-from-one-struct.md) | A tool's schema and its handler come from one struct | Superseded by [0010](0010-tool-schemas-from-prompt-files.md) |
| [0007](0007-skills-follow-the-open-spec.md) | Skills follow the open Agent Skills spec, parsed without a serde YAML crate | Partially superseded by [0011](0011-skills-frontmatter-parsing-moves-to-prompts.md) |
| [0008](0008-admission-bar-and-workspace-shape.md) | The admission bar is "would I use this again", and the crate is a workspace | Accepted |
| [0009](0009-streaming-is-the-provider-primitive.md) | Streaming is the provider primitive, and it is public | Accepted |
| [0010](0010-tool-schemas-from-prompt-files.md) | A tool's schema and handler come from a prompt file, not a schemars struct | Accepted |
| [0011](0011-skills-frontmatter-parsing-moves-to-prompts.md) | Skills' frontmatter parsing moves into `grizzly-agent-prompts` | Accepted |
| [0012](0012-generated-tool-spec-through-tooldefinition-only.md) | Generated tools expose `spec()` only through `ToolDefinition` | Accepted |
