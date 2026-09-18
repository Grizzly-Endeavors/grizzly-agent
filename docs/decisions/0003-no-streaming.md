# ADR-0003 — No streaming until a consumer proves it needs it

**Status:** Superseded by [ADR-0009](0009-streaming-is-the-provider-primitive.md) (2026-09-18)

## Context

Not one of the ten surveyed projects streams. Every provider client sets `stream: false` — `Ursix/src/llm/ollama.rs:132` and `entity-extractor`'s response types make it explicit, and `gameservers`, `career-scanner`, and `residuum` all block for the full response. This is a standing decision that has been made repeatedly and independently, not an omission.

The delivery surfaces explain most of it. `gameservers` posts replies to Discord, `career-scanner` runs a batch extraction pipeline, and `entity-extractor` sweeps records through a DAG — none of these has a place to put a token as it arrives. `residuum` is the one project with an interactive surface, and it produces the *feel* of streaming by publishing whole text blocks over its event bus between tool calls (`src/agent/turn.rs:152`), which turned out to be enough.

Against that, streaming is the one gap on the list that changes the provider trait's shape rather than adding beside it. Retrofitting it after consumers exist means a coordinated break across every repo that depends on this crate. That argument is real, and it is why this ADR exists rather than the question simply going unasked.

## Decision

**The provider trait is request/response. No streaming in v1.**

The deciding factor is that the alternative buys an unknown. Designing a trait "with streaming in mind" before any consumer has streamed means guessing at the shape — whether deltas are aggregated for the caller or exposed raw, how tool-call fragments accumulate across chunks, what a partial turn means to the loop — with no working example to check the guess against. A speculative seam that turns out wrong is worse than no seam: it has to break anyway, and in the meantime every consumer pays for machinery none of them use.

If a consumer needs streaming, it gets built then, shaped by that consumer's actual requirements, and the break is taken knowingly with a real design to justify it.

### Rejected: add `complete_stream` alongside `complete` now

Two methods where every implementation stubs one out. Each new provider adapter would have to decide what to do with a method no caller invokes, and the honest answer — `unimplemented!()` — is denied by this crate's lint config for good reason.

### Rejected: make `complete` return a stream that usually yields one item

Uniform in principle, and it does avoid the break. But it makes the common case — one request, one response — the awkward one, forcing every caller to collect a stream to get the thing they actually wanted. Optimizing the API for the case nobody currently has, at the expense of the case everybody has, is the wrong trade.

## Consequences

- Adding streaming later is a breaking change to the provider trait. This is accepted deliberately, with the reasoning above; it is not an oversight to be discovered later.
- Consumers wanting incremental output can do what `residuum` does — emit whole blocks between tool-loop iterations. The turn loop's callback seams already make this possible without provider streaming.
- The `Usage` type is unaffected, since token counts arrive with the completed response either way (see ADR-0004).
