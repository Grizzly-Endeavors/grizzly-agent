# ADR-0001 — The admission test is two real consumers, not usefulness

**Status:** Accepted (2026-08-09)

## Context

This crate exists because the same primitives were re-solved across a dozen Rust projects that call LLMs. `docs/design/primitive-index.md` surveys all of them, and the duplication turned out to be literal rather than conceptual: `project-residuum/src/models/retry.rs` and `residuum-code/src/core/providers/retry.rs` are byte-for-byte identical, and `grizzly-gameservers/src/discord/chunking.rs` carries a doc comment saying it was "ported from the residuum Discord adapter." Four projects independently wrote the same provider trait.

That is a strong case for a shared crate. It is also exactly how a shared crate becomes a framework. Every primitive in the survey was useful to the project that wrote it — usefulness cannot be the filter, because it admits everything. A toolkit that admits everything is one its own author has to fight, at which point the duplication it replaced was cheaper.

The survey also showed that "useful" and "generalizable" come apart in a specific way. `career-scanner`'s résumé projection, `gameservers`' access tiers, and `residuum`'s skills-and-projects context assembly are all good code. None of them would have been written the same way by a second project, because each encodes a decision about a product rather than a fact about agents.

## Decision

A primitive is admitted only when all three hold:

1. **Two real consumers exist.** Not one consumer and a hypothetical second. The index records how many projects independently needed each thing, so this is answerable from evidence rather than argument.
2. **No product decision is embedded.** Which model, which storage, what a tool may do, what a good answer looks like — these belong to the consumer. If generalizing something requires adding a config knob to represent a choice, that choice probably is not ours.
3. **The consumer cannot do it better locally.** Some things are cheaper written twice than abstracted once.

Where two projects need the same thing but shape it differently, what is admitted is the **trait**, not the implementation.

**Rejection is the expected outcome**, and a rejection gets written down. An unrecorded "no" gets re-litigated every few months by whoever next notices the gap; a recorded one is answerable in a link. ADR-0002 through ADR-0004 are the first three.

v1 admits: provider connectors, message and conversation types, typed errors, retry and backoff, the turn loop, tool definition and dispatch, structured output, and skills.

## Consequences

- **Memory and retrieval are deferred, not rejected.** `residuum`'s hybrid BM25 + vector search is the largest self-contained subsystem in the survey and clearly belongs here eventually. It lands after v1, behind a feature flag, once the spine is stable enough that a second consumer can be built against it. Deferring it keeps `tantivy` and `sqlite-vec` out of every consumer's dependency tree in the meantime.
- **The crate is single-crate and feature-gated.** Heavier pieces arrive as optional features rather than a workspace split. Cargo's workspace lint inheritance is all-or-nothing, so splitting later means duplicating the lint block verbatim in every member; that cost is worth paying only when a genuine second artifact appears.
- **Some duplication survives on purpose.** `gameservers` keeps its approval gating and untrusted-text fencing local for now — that work is excellent and safety-critical, but it has exactly one consumer, so admitting it would be an argument-based decision in a crate that has committed to evidence-based ones. It becomes admissible the moment a second agent needs it.
- The index is a snapshot of 2026-08-09 and will go stale. It is evidence for decisions already made, not a live inventory to maintain.
