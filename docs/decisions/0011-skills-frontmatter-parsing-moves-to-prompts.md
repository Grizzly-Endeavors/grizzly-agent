# ADR-0011 — Skills' frontmatter parsing moves into `grizzly-agent-prompts`

**Status:** Accepted (2026-09-18)
**Supersedes (in part):** [ADR-0007](0007-skills-follow-the-open-spec.md)

## Context

ADR-0007 chose `yaml-rust2` for skills' own frontmatter parsing, hand-mapping the six Agent Skills spec fields. That choice — the library, the six fields, their validation rules, unknown-key preservation, and every other consequence in ADR-0007 — is unchanged and still in force; this record only supersedes where the parsing step lives.

A `SKILL.md` file and a `grizzly-agent-prompts` prompt file are both Markdown with YAML frontmatter, and now that the crate is a workspace with `grizzly-agent-prompts` as a member, the split-and-parse step — separate the frontmatter block from the body, hand the frontmatter to `yaml-rust2` — is identical work with nothing schema-specific about it. Duplicating roughly thirty lines of that step in a `grizzly-agent-skills` crate alongside an already-present copy in prompts would mean two YAML dependencies in the workspace and two implementations of the same split-and-parse to keep in sync.

## Decision

**The default (non-`codegen`, non-`verify`) face of `grizzly-agent-prompts` owns splitting a Markdown file into frontmatter and body and parsing that frontmatter with `yaml-rust2`.** `grizzly-agent-skills` depends on prompts' default face alone — not `codegen`, not `verify` — and builds its own six-field validation on top of that shared step. This keeps one YAML dependency and one frontmatter implementation in the workspace, while skills' schema-specific validation (name matches its directory, `allowed-tools` in either accepted form, and so on) stays where it always was, in the skills crate.

Skills was kept out of the prompts crate itself, rather than folding the other way, because skills runs at runtime and needs core's tool and loop types (`ToolHandler`, `ToolSet`), while prompts runs at build time and must stay free of core — folding skills into prompts would pull core into every consumer's build script.

### Rejected: duplicate the split-and-parse step in `grizzly-agent-skills`

What ADR-0007 implied by choosing `yaml-rust2` for skills without a workspace to share it across. Rejected now that the sharing is nearly free: it costs skills one light dependency on prompts' default face and avoids a second `yaml-rust2` pin and a second hand-mapped split-and-parse routine to keep in sync with the first.

## Consequences

- `grizzly-agent-skills` depends on `grizzly-agent-prompts`, but only its default features — the dependency does not pull in codegen's build-tree walking or verify's call-site cross-checking, and it does not pull in core through prompts (prompts stays core-free regardless of what depends on it).
- ADR-0007's field list, validation rules, unknown-key handling, and the `yaml-rust2` choice itself are all still current and are not restated here — read ADR-0007 for them.
- Everything in ADR-0007's "Consequences" about policy scope (skills loads and validates; it does not implement progressive-disclosure policy or enforce `allowed-tools`) is unchanged and still describes this crate.
