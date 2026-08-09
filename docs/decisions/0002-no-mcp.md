# ADR-0002 — MCP stays out, permanently

**Status:** Accepted (2026-08-09)

## Context

`project-residuum` is the only surveyed project with MCP support, and it is good: `src/mcp/client.rs:30` handles both stdio and Streamable HTTP transports over `rmcp`, and `src/mcp/registry.rs:99` does pure desired-versus-running diff reconciliation with reference-counted server lifecycle so concurrent sub-agents sharing a project's servers only spin them down when the last one releases. `poe2/mcp` implements a server on the Python side.

MCP is also squarely agent-shaped. On the surface it is exactly the kind of connective tissue a foundational agent crate would carry, and it was on the shortlist for that reason.

## Decision

**MCP is out of scope, and not on a roadmap.** Projects that need it depend on `rmcp` directly or hand-roll the transport.

This is a permanent boundary rather than a deferral. ADR-0001 defers memory because it will eventually meet the admission test; MCP is excluded because re-exporting it would not add anything a consumer cannot get from `rmcp` itself.

The one real gap the survey found — MCP tools and local tools needing to look identical to the turn loop — is solved without depending on MCP at all. `job-finder/src/browser/mcp_client.py:17` demonstrates the shape: the loop only knows a `ToolProvider`, and whether a call crosses a subprocess boundary is that provider's business. This crate defines that trait. An `rmcp`-backed implementation of it is about thirty lines in the consumer, and belongs there.

### Rejected: ship an `rmcp` wrapper behind a feature flag

The feature flag makes the dependency optional, so the cost looks like zero. It is not. A wrapper has to track `rmcp`'s API across versions, and every consumer inherits this crate's opinion about which `rmcp` version and which transports are supported — precisely when a consumer with an unusual transport need is the one who most wants to reach the underlying library. Wrapping a well-designed crate to add nothing is how a toolkit becomes something to route around.

### Rejected: ship the registry's reconciliation logic without the client

The reference-counted, diff-based lifecycle in `residuum`'s registry is genuinely more sophisticated than most MCP client code in the wild, and it is the part with real design content. But it is meaningless without concrete servers to reconcile, which puts `rmcp` back in the dependency tree through the back door.

## Consequences

- `residuum` keeps its MCP code. Nothing is extracted from it, and the reference-counted registry stays where it is.
- The `ToolProvider` trait must be good enough that an `rmcp` adapter is trivial to write against it. If writing that adapter turns out to be awkward, the trait is wrong — not this decision.
- A future contributor will notice MCP is missing and read this as an oversight. This ADR is the answer, and it is deliberately worded as permanent so the question closes rather than reopening each time.
