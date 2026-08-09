# grizzly-agent

## Getting started

```sh
cargo run
```

## Development

```sh
cargo test --quiet                                          # tests
cargo fmt --all                                             # format
cargo clippy --all-targets --all-features -- -D warnings    # lint
cargo deny check                                            # audit dependencies
```

With [just](https://github.com/casey/just) installed, `just ci-local` runs all four.

Git hooks enforce the same checks on every commit. Install them once with `./.githooks/install.sh`.

Conventions this project follows — and the reasoning behind them — are in [CLAUDE.md](CLAUDE.md) and [docs/toolkit.md](docs/toolkit.md).
