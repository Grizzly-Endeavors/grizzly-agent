//! The codegen face: parse and validate a prompt directory, then emit the
//! generated `prompts.rs` module.
//!
//! Everything under here is gated behind the `codegen` feature (declared once,
//! on the `mod codegen` line in `lib.rs`). [`load`] produces the validated
//! [`PromptTree`]; [`PromptCodegen`] (and the [`emit`]/[`generate`] free
//! functions it wraps for the default crate path) turn that tree into Rust
//! source.

mod builder;
mod emit;
mod error;
mod ident;
mod model;
mod parse;
mod validate;

use std::path::Path;

pub use builder::{DEFAULT_CRATE_PATH, PromptCodegen};
pub use error::PromptError;
pub use model::{
    Annotations, BodySegment, Param, ParamType, PromptFile, PromptKind, PromptTree, ToolSchema,
    ToolSchemaRef, UsedBy, Variable,
};
pub use validate::load;

/// One-argument build-script entry point: validates and writes the generated
/// module for `prompts_dir`, targeting the default crate path
/// ([`DEFAULT_CRATE_PATH`]). Equivalent to `PromptCodegen::new(prompts_dir).emit()`.
///
/// # Errors
///
/// See [`PromptCodegen::emit`].
pub fn emit(prompts_dir: &Path) -> Result<(), PromptError> {
    PromptCodegen::new(prompts_dir).emit()
}

/// Loads, validates, and renders the generated module for `prompts_dir` as a
/// source string, targeting the default crate path. Pure — used by tests and
/// by [`emit`]. Equivalent to `PromptCodegen::new(prompts_dir).generate()`.
///
/// # Errors
///
/// See [`PromptCodegen::generate`].
pub fn generate(prompts_dir: &Path) -> Result<String, PromptError> {
    PromptCodegen::new(prompts_dir).generate()
}
