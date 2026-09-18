# ADR-0009 — Streaming is the provider primitive, and it is public

**Status:** Accepted (2026-09-18)
**Supersedes:** [ADR-0003](0003-no-streaming.md)

## Context

ADR-0003 held the line at request/response because no surveyed consumer streamed, and because streaming changes the provider trait's shape rather than adding beside it — a real cost, worth avoiding until a consumer's actual requirements could shape the seam instead of a guess.

`gantry` has since hit the case ADR-0003 flagged as the deciding risk in the other direction: a non-streamed request is silent for the whole generation, and long reasoning generations were killed as idle by a proxy and retried to exhaustion. Every provider streams on the wire regardless of what the client-facing API looks like, specifically to keep connections alive through tunnels and proxies with idle timeouts. That is no longer a hypothetical a design has to guess at — it is the reason a real request failed.

## Decision

**The provider trait's one required operation opens a completion stream**, not a request/response call. `Provider` is object-safe and returns a boxed `Send` stream of `CompletionEvent`s — text deltas, reasoning deltas, tool-use start and argument-delta events, usage, and a closing `finished` event — or fails before the stream opens. A whole `Completion` is derived from the stream by one accumulator in core, not re-implemented per provider. `Model` exposes both `complete` (buffers the stream, retries the whole request on any failure) and `stream` (yields events as they arrive, retrying only failures before the first delivered event, since output already handed to the caller cannot be un-sent).

The event stream is public, not a hidden transport detail: `Model::stream` and the turn loop's run observer both expose it, so an interactive agent can render output live.

### Rejected: request/response only, add streaming later when a consumer needs it (status quo)

This is ADR-0003 as written, and it already produced the failure it accepted as a risk: a proxy killing an idle non-streamed request. Adding streaming later, after providers and consumers exist, means a coordinated breaking change to a trait every provider implements — cheaper to take now, before there are any.

### Rejected: streaming as a hidden transport detail, request/response as the only public API

Solves the idle-connection problem without a public API change. Rejected because it was the earlier plan for exactly this design and was dropped: adding public streaming later still means changing the provider contract every provider implements, so it defers the same cost without avoiding it, while denying `gantry`'s render-as-it-arrives case entirely.

## Consequences

- `CompletionRequest`, `CompletionEvent`, and the accumulator's block-ordering, argument-reassembly, and max-tokens-truncation rules are now core's contract, tested with a scripted provider rather than live network calls.
- The retry rule for `stream` — only before the first delivered event — is the accepted cost of exposing partial output live: a caller that has already rendered a partial reply cannot have a retry take it back.
- `Usage`'s shape is unaffected: usage still arrives as a completion field (now possibly emitted more than once mid-stream, with later values superseding earlier ones), not something this crate estimates.
- Consumers that do not want to render output live still get it for free through `complete`, which is what most callers and every eval use.
