# grizzly-agent

The one dependency a Rust project adds to build an agent or otherwise work with LLMs, so each new project reuses what the last one built instead of re-solving it. It is a dependency, not a framework and not an application — it provides the pieces an agent is assembled from and leaves policy to the consumer.

## Scope

The admission bar is **"would I want to use this again?"** — not "do two projects already need it." Waiting for a second consumer to prove a primitive is worth extracting only guarantees the second project starts with a migration and a copy that has already drifted from the first. A primitive is admitted when it embeds no product decision and the consumer could not do it better locally; where two projects need the same thing but shape it differently, what belongs here is the trait, not the implementation. [ADR-0008](docs/decisions/0008-admission-bar-and-workspace-shape.md) has the reasoning.

Anything encoding a product decision — which model, what a tool may do, how a conversation is persisted, what a good answer looks like — belongs in the consumer, behind a trait this crate defines but does not implement.

**Deliberately out**, each with a recorded reason so the question closes instead of recurring:

| Not here | Why |
| --- | --- |
| MCP | Permanent. Depend on `rmcp` directly — a wrapper adds nothing. ([ADR-0002](docs/decisions/0002-no-mcp.md)) |
| Token counting | Providers already return exact counts; this crate reports them rather than estimating. ([ADR-0004](docs/decisions/0004-provider-reported-token-counts.md)) |
| Transcript trimming and session storage, telemetry/recording, structured-output repair, memory and retrieval, cost tracking | Each is a candidate for later work; none is needed for the current shape to be complete. |

## Workspace

This repository is a Cargo workspace; every member is versioned together. Four packages exist today:

| Package | Responsibility |
| --- | --- |
| `grizzly-agent` | **Facade.** Re-exports the runtime crates behind features. The one line a consumer adds for runtime use. |
| `grizzly-agent-core` | Conversation types, the error taxonomy, and the tool model. No HTTP. |
| `grizzly-agent-prompts` | Prompt-file frontmatter parsing (default), codegen (`codegen` feature, a build-dependency), and call-site verification (`verify` feature, a dev-dependency). Does not depend on core. |
| `grizzly-agent-eval` | `ResponseEval` and the shared case metadata, verdict, aggregation, and report core it and `AgentEval` feed. Depends on core, not on providers. |

The rest of the workspace — provider clients, a turn loop, Agent Skills, and AgentEval — lands incrementally on top of this foundation, per the design and phase plan in [`docs/design/workspace/`](docs/design/workspace/). Lints, toolchain, and deny policy live once at the workspace root; every member inherits the lint table unchanged via `lints.workspace = true`.

**Consumption rule.** A consumer depending on more than one workspace member — for example the facade for runtime use and `grizzly-agent-prompts` on its build edge — must point every member at the same git revision. Cargo otherwise resolves two copies of `grizzly-agent-core`, and a generated `ToolSpec` from one copy will not type-check against the facade's re-export from the other.

## Use it

```toml
[dependencies]
grizzly-agent = { git = "ssh://git@github.com/Grizzly-Endeavors/grizzly-agent.git" }
```

A bare dependency with no features enabled gets `grizzly-agent-core` alone — conversation types and errors, with no provider, skill, or eval machinery pulled in.

## Development

```sh
cargo test --workspace --quiet                                          # tests (default features)
cargo test --workspace --all-features --quiet                           # tests (all features — exercises prompts' codegen/verify)
cargo fmt --all                                                         # format
cargo clippy --workspace --all-targets --all-features -- -D warnings    # lint
cargo deny check                                                        # audit dependencies
cargo doc --workspace --no-deps --all-features --open                   # read the public API
```

With [just](https://github.com/casey/just) installed, `just ci-local` runs the full gate (format, lint, test, deny, and the provider feature matrix) and `just doc` runs the last command above.

Git hooks enforce the same checks on every commit. Install them once with `./.githooks/install.sh`.

Conventions this project follows — and the reasoning behind them — are in [CLAUDE.md](CLAUDE.md). Decisions that were close calls are recorded in [docs/decisions/](docs/decisions/).
