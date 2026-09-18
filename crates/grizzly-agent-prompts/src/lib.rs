//! Prompt-file frontmatter parsing, codegen, and call-site verification.
//!
//! A prompt file is Markdown with YAML frontmatter: the frontmatter declares
//! an id, a type (`prompt`, `tool`, or `params`), and human-facing
//! annotations; the body is the verbatim text sent to the model. This crate
//! has three dependency-isolated faces, selected by cargo feature:
//!
//! - **default:** [`parse_frontmatter`] splits a file into its frontmatter
//!   and body and parses the frontmatter into a `yaml-rust2` document. No
//!   codegen, no filesystem walking — this is what `grizzly-agent-skills`
//!   builds its own `SKILL.md` parsing on.
//! - **`codegen`** (a consumer's build-dependency): [`load`] loads and
//!   validates a prompt directory into a [`PromptTree`]; [`PromptCodegen`]
//!   renders that tree into the generated Rust module and, from a build
//!   script, writes it and registers the tree for rebuild-on-change.
//! - **`verify`** (a consumer's dev-dependency): [`verify_annotations`]
//!   cross-references each prompt's `used_by` annotations and id against the
//!   consumer's source tree.
//!
//! This crate does not depend on `grizzly-agent-core`. Generated code
//! references core's types — `ToolSpec`, `ToolDefinition`, `NoParams`, and
//! the hidden `serde_json` re-export — through a configurable crate path
//! (default `grizzly_agent`, the facade), set with
//! [`PromptCodegen::crate_path`]. A consumer depending on core directly sets
//! it to `grizzly_agent_core`.

mod frontmatter;

pub use frontmatter::{Frontmatter, FrontmatterError, parse_frontmatter, split_frontmatter};

#[cfg(feature = "codegen")]
mod codegen;
#[cfg(feature = "codegen")]
pub use codegen::{
    Annotations, BodySegment, DEFAULT_CRATE_PATH, Param, ParamType, PromptCodegen, PromptError,
    PromptFile, PromptKind, PromptTree, ToolSchema, ToolSchemaRef, UsedBy, Variable, emit,
    generate, load,
};

#[cfg(feature = "verify")]
mod verify;
#[cfg(feature = "verify")]
pub use verify::{VerifyError, VerifyReport, verify_annotations};
