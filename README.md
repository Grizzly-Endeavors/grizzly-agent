# grizzly-agent

Foundational primitives for building LLM agents in Rust. It is a dependency, not a framework and not an application — it provides the pieces an agent is assembled from and leaves policy to the consumer.

## Scope

The boundary is the whole point of this crate, so it is stated up front:

**In scope** — a primitive earns a place here when at least two independent projects need it and neither can define it better locally. Provider connectors, conversation and content types, tool definition and dispatch, the turn loop, and the traits that let a consumer plug in its own storage and policy.

**Out of scope** — anything that encodes a product decision. Which model to use, what a tool is allowed to do, how a conversation is persisted, what a good answer looks like. Those belong in the consumer, behind a trait this crate defines but does not implement.

The test for a proposed addition is not "would this be useful?" — almost anything would. It is "would a second project have written this the same way?" If the answer is no, it belongs in the project that needs it.

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
