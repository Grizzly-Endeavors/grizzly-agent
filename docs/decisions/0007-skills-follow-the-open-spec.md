# ADR-0007 — Skills follow the open Agent Skills spec, parsed without a serde YAML crate

**Status:** Accepted (2026-08-09); partially superseded by [ADR-0011](0011-skills-frontmatter-parsing-moves-to-prompts.md) (2026-09-18), which moves the split-and-parse step described here into `grizzly-agent-prompts`. The spec fields, their validation, the `yaml-rust2` choice, and the policy-scope consequences below are still current.

## Context

Two decisions are tangled here, and separating them is most of the work.

**There is not one skills spec — there are two.** The open **Agent Skills standard** (published 2025-12-18, now hosted at `agentskills.io/specification` independently of Anthropic) defines exactly six frontmatter fields: `name`, `description`, `license`, `compatibility`, `metadata`, and `allowed-tools`. Anthropic's own products then diverge from it:

- Claude Code accepts seventeen-plus additional fields locally, but enforces the six at packaging and upload boundaries.
- The Skills API relaxes the directory-name match to case- and underscore-insensitive, where the open spec requires an exact match.
- Anthropic's docs add rules absent from the standard: no XML tags, and a reserved-word ban on `anthropic` and `claude`.
- The Agent SDK ignores `allowed-tools` entirely; Claude Code honours it for one turn.

No layer defines a `version` field. No official JSON Schema exists to vendor.

**Separately, the serde-integrated YAML ecosystem is in poor health.** `serde_yaml` publishes as `0.9.34+deprecated`. `serde_yml` is deprecated and forwards to `noyalib`. `serde_norway`, the mature `serde_yaml` fork, last released 2024-12-21 — eighteen months before this decision. `saphyr-serde` is a `0.0.0` placeholder. The maintained options are `noyalib` (0.0.18) and `saphyr` (0.0.11), both pre-1.0.

## Decision

**Match the open standard.** Six fields, validated to the standard's constraints: `name` is 1–64 characters of lowercase alphanumerics and hyphens, with no leading, trailing, or consecutive hyphen, matching its parent directory exactly; `description` is 1–1024 characters; `compatibility` is at most 500; `metadata` is a flat string-to-string map.

Anthropic's product-specific rules are not enforced. Banning the word "claude" in a skill name is a policy of one vendor's hosting, not a property of the format, and a crate that enforced it would reject valid skills.

**Unknown frontmatter keys are preserved, not rejected.** A Claude Code skill using `argument-hint` must still load. Strictness belongs at the authoring and packaging boundary, which this crate is not; a loader that rejects unknown keys cannot read the skills people actually have.

**Parse with `yaml-rust2`, mapping the six fields by hand.** Fifty million downloads, six releases in the last twelve months, YAML 1.2 compliant, actively maintained.

### Rejected: `serde_norway`

Derive works, the code is a third the size, and the lineage is mature. Rejected because eighteen months without a release is precisely the signal this project's rules say to treat as a red flag. YAML is a stable surface so it might never matter — but if it does, the remedy is forking a fork, and every consumer of this crate inherits that.

### Rejected: `noyalib` and `saphyr`

Both actively developed, and `noyalib` has full serde integration. Rejected because a `0.0.x` version carries no stability promise, and pre-1.0 churn in a foundational crate's dependency lands on every consumer. `saphyr` additionally has no usable serde companion, so it means hand-mapping anyway — the same work as `yaml-rust2` with less maturity behind it.

### Rejected: hand-rolling a YAML subset parser

The frontmatter is small enough to make this tempting. It is also how people discover that YAML has multi-line scalars, quoting rules, and comments. Not worth it.

## Consequences

- The hand-mapping is roughly sixty lines against a closed schema. The standard describes the field set as fixed, so this does not grow with normal use — and if the standard adds a field, adding it here is deliberate rather than automatic.
- Skills are behind the `skills` feature, so a consumer that does not use them takes no YAML dependency at all.
- This crate loads and validates skills. It does not implement the progressive-disclosure *policy* — deciding when to promote a skill from index to body is the consumer's call, and this crate exposes the levels rather than choosing between them.
- Discovery order and precedence (enterprise over personal over project, plugin namespacing) are Claude Code runtime behaviour, not part of the format. They are the consumer's to implement.
- The spec is roughly eight months old and moving. This records what it said on 2026-08-09.
