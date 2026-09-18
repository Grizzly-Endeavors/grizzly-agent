# ADR-0008 — The admission bar is "would I use this again", and the crate is a workspace

**Status:** Accepted (2026-09-18)
**Supersedes:** [ADR-0001](0001-scope-boundary.md)

## Context

ADR-0001 admitted a primitive only once two real consumers independently needed it. That bar produced the crate's first shape correctly, but it has an inherent lag: a primitive only clears it after a second project has already re-solved the problem, which means the second project starts with a migration and a copy that has already drifted from the first. Building agents is the most common thing done across the projects this crate serves, so waiting for evidence of duplication is waiting for duplication to happen.

Two sibling projects meanwhile built exactly the pieces this crate was missing, each locked inside its own repo: `grizzly-gameservers`'s `prompt-lib` (prompt compilation, tool schemas, call-site verification) and `gantry`'s eval suite (repeats, pass thresholds, failure categories, aggregation, a self-explaining report). Neither needed a second consumer to be worth extracting — each is something a third project would clearly want, and the two-consumer test would have said no to both until one arrived.

Separately, ADR-0001's consequences committed the crate to a single feature-gated crate specifically to avoid Cargo's all-or-nothing workspace lint inheritance. That cost was worth paying while the codebase was small. It no longer is: the crate is growing a provider layer, a tool model, a turn loop, prompts, skills, and evals — six components with materially different dependency edges (HTTP only behind provider features, a YAML parser only for prompts, `tokio_util` only where cancellation is used). A single crate with a feature per component reaches hundreds of feature combinations, and `--all-features` CI stops catching a component that silently needs another one enabled. Crate boundaries make layering compiler-enforced in a way `pub(crate)` inside one crate cannot: eval cannot reach into loop internals, the loop cannot reach into a provider's.

## Decision

**The admission bar is "would I want to use this again?"** — not "do two projects already need it." A primitive is admitted when it would clearly be reached for by a third project, embeds no product decision, and the consumer could not do it better locally. Two real consumers remain strong evidence when they exist, but their absence is no longer disqualifying on its own.

**The crate becomes a Cargo workspace of six packages**, all versioned together: `grizzly-agent` (the facade, re-exporting the runtime crates behind features), `grizzly-agent-core` (conversation types, errors, the provider trait, the tool model, the turn loop — no HTTP), `grizzly-agent-providers` (concrete provider clients), `grizzly-agent-prompts` (frontmatter parsing, codegen, verification), `grizzly-agent-skills` (Agent Skills), and `grizzly-agent-eval` (ResponseEval and AgentEval). Dependency direction is strict: `providers → core`, `skills → core + prompts`, `eval → core`, `facade → everything runtime`. Nothing depends on the facade, and core depends on no other workspace member. Lints, toolchain, and deny policy live once at the workspace root; every member inherits the lint table unchanged via `lints.workspace = true`.

### Rejected: keep the two-consumer test, extract gameservers' and gantry's pieces only once each has a second user

This is what ADR-0001 would produce as written. Rejected because it is the exact failure mode the sibling projects already demonstrate: both pieces are good, reusable, product-decision-free code that a third project would want, sitting unextracted purely because no second consumer exists yet to trigger the rule.

### Rejected: stay single-crate, add features for the new components

Cheaper in the short run — no new manifests, one lint block to write. Rejected because the crate has outgrown what a shared feature namespace can enforce: prompt codegen running in a consumer's build script would compile core and whatever runtime features happen to be on for the host, purely to run a parser, and nothing would stop eval code from calling into loop internals it should not touch.

## Consequences

- Every member manifest becomes more explicit — dependencies that used to live in one `Cargo.toml` behind a feature flag now live in the crate that actually needs them, which is the point of the split.
- The workspace lint table cannot be relaxed per member; a lint that turns out wrong for one crate needs a scoped `#[expect(..., reason = "...")]` at the item level, same as before.
- Rejection under the new admission bar still gets written down. An unrecorded "no" gets re-litigated; a recorded one is answerable in a link.
- `docs/archive/primitive-index.md`, the evidence base ADR-0001 was decided from, remains as historical record of the projects surveyed; it is not required to justify new admissions going forward.
