# grizzly-agent

Shared Rust primitives for building agents and calling LLMs. Provides reusable pieces and traits; leaves product policy (model choice, tool permissions, persistence, scoring policy) to the consumer.

## Scope

- **Admission bar:** a primitive belongs here when it embeds no product decision and a consumer could not do it better locally. Where consumers shape behavior differently, expose the trait rather than a concrete policy.
- **Out of scope:** MCP (depend on `rmcp` directly) and local token estimation (providers report exact token counts).

## Workspace

All members are versioned together under `crates/`:

| Package | Responsibility |
| --- | --- |
| `grizzly-agent` | Facade re-exporting runtime crates behind feature flags. |
| `grizzly-agent-core` | Conversation types, error taxonomy, tool model, model calls, turn loop, and `Agent` (no HTTP). |
| `grizzly-agent-providers` | Anthropic and OpenAI-compatible HTTP clients behind feature flags. |
| `grizzly-agent-prompts` | Prompt-file frontmatter parsing, build-time codegen (`codegen`), and call-site verification (`verify`). |
| `grizzly-agent-eval` | `ResponseEval`, `AgentEval`, and shared scoring/report core. |
| `grizzly-agent-skills` | Agent Skills index and activation per the [Agent Skills specification](https://agentskills.io/specification). |

**Consumption rule:** A consumer depending on more than one workspace member (for example `grizzly-agent` at runtime and `grizzly-agent-prompts` as a build-dependency) must pin every member to the same git revision. Otherwise Cargo resolves two copies of `grizzly-agent-core` and generated `ToolSpec` types will not match the facade's re-exports.

## Use it

```toml
[dependencies]
grizzly-agent = { git = "ssh://git@github.com/Grizzly-Endeavors/grizzly-agent.git" }
```

With no features enabled, `grizzly-agent` pulls only `grizzly-agent-core`.

## Development

```sh
cargo test --workspace --quiet
cargo test --workspace --all-features --quiet
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo deny check
cargo doc --workspace --no-deps --all-features --open
```

`just ci-local` runs the full gate (format, lint, test, deny, and the provider feature matrix). Install pre-commit hooks with `./.githooks/install.sh`. Conventions live in [AGENTS.md](AGENTS.md).
