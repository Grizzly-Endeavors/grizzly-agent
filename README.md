# grizzly-agent

Foundational primitives for building LLM agents in Rust. It is a dependency, not a framework and not an application — it provides the pieces an agent is assembled from and leaves policy to the consumer.

## Scope

The boundary is the whole point of this crate, so it is stated up front.

The test for a proposed addition is not "would this be useful?" — almost anything would. It is **"would a second project have written this the same way?"** A primitive is admitted only when two real consumers need it, it embeds no product decision, and the consumer could not do it better locally. Where two projects need the same thing but shape it differently, what belongs here is the trait, not the implementation. [ADR-0001](docs/decisions/0001-scope-boundary.md) has the reasoning; [`docs/design/primitive-index.md`](docs/design/primitive-index.md) is the evidence it was decided from.

**In v1** — provider connectors, message and conversation types, typed errors, retry and backoff, the turn loop, tool definition and dispatch, structured output, and skills.

**Deliberately out**, each with a recorded reason so the question closes instead of recurring:

| Not here | Why |
| --- | --- |
| MCP | Permanent. Depend on `rmcp` directly — a wrapper adds nothing. ([ADR-0002](docs/decisions/0002-no-mcp.md)) |
| Streaming | No consumer streams today, and a speculative seam guessed wrong is worse than none. ([ADR-0003](docs/decisions/0003-no-streaming.md)) |
| Token counting | Providers already return exact counts; this crate reports them rather than estimating. ([ADR-0004](docs/decisions/0004-provider-reported-token-counts.md)) |
| Memory and retrieval | Deferred, not rejected. Lands after v1 behind a feature flag. |
| Evaluation harness | No proven shape to extract yet. |

Anything encoding a product decision — which model, what a tool may do, how a conversation is persisted, what a good answer looks like — belongs in the consumer, behind a trait this crate defines but does not implement.

## Use it

```toml
[dependencies]
grizzly-agent = { git = "ssh://git@github.com/Grizzly-Endeavors/grizzly-agent.git" }
```

## Development

```sh
cargo test --quiet                                          # tests
cargo fmt --all                                             # format
cargo clippy --all-targets --all-features -- -D warnings    # lint
cargo deny check                                            # audit dependencies
cargo doc --no-deps --all-features --open                   # read the public API
```

With [just](https://github.com/casey/just) installed, `just ci-local` runs the first four and `just doc` runs the last.

Git hooks enforce the same checks on every commit. Install them once with `./.githooks/install.sh`.

Conventions this project follows — and the reasoning behind them — are in [CLAUDE.md](CLAUDE.md) and [docs/toolkit.md](docs/toolkit.md). Decisions that were close calls are recorded in [docs/decisions/](docs/decisions/).
