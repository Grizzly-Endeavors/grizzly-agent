//! Agent Skills for `grizzly-agent`: loading, indexing, and runtime
//! activation, per the open
//! [Agent Skills specification](https://agentskills.io/specification).
//!
//! Three layers, matching the spec's own progressive disclosure:
//!
//! - **Format** ([`parse_skill`]): parses one `SKILL.md` file's frontmatter
//!   into its six spec fields, validated to the spec's constraints. Unknown
//!   frontmatter keys are preserved, not rejected; no vendor-specific rules
//!   are enforced.
//! - **Index** ([`SkillIndex`]): scans an ordered list of directories for
//!   skills. Earlier directories win on a name collision; duplicates and
//!   invalid skills are reported as diagnostics and skipped, never fatal to
//!   the scan.
//! - **Activation** ([`SkillState`], [`activate_skill_tool`],
//!   [`deactivate_skill_tool`], [`SkillsSection`]): shared state behind two
//!   [`grizzly_agent_core::ToolHandler`]s and a
//!   [`grizzly_agent_core::DynamicSection`] that a consumer adds to their
//!   `ToolSet` and `Agent` for progressive disclosure with no further code.
//!
//! `allowed_tools` on [`Skill`] is parsed and exposed, not enforced:
//! activating a skill never changes what a `ToolSet` advertises. A consumer
//! that wants a different activation policy uses the format and index
//! layers alone.
//!
//! This crate depends on `grizzly-agent-prompts`' default face only — no
//! codegen, no build-time filesystem walking — for the split-and-parse step
//! shared with prompt files. It reads skill directories at scan and
//! activation time and does not cache across processes.

mod activation;
mod format;
mod index;

pub use activation::{SkillState, SkillsSection, activate_skill_tool, deactivate_skill_tool};
pub use format::{Skill, SkillError, parse_skill};
pub use index::{InvalidSkillReason, SkillDiagnostic, SkillIndex};
