//! The public codegen entry point: a builder from a prompt directory, an
//! optional non-default crate path, and a method that generates or emits.

use std::path::{Path, PathBuf};

use super::emit;
use super::error::PromptError;

/// The crate path generated code references core's types through when none is
/// set: the facade, so an unconfigured consumer's generated module compiles
/// against a bare `grizzly-agent` dependency.
pub const DEFAULT_CRATE_PATH: &str = "grizzly_agent";

/// Builds and emits the generated prompts module for a prompt directory.
///
/// The one-argument [`emit`](super::emit) and [`generate`](super::generate)
/// free functions cover the default crate path (`grizzly_agent`, the facade)
/// so an existing build script needs no change. Reach for this builder to
/// target a different one — for example `grizzly_agent_core`, for a consumer
/// depending on core directly instead of through the facade.
pub struct PromptCodegen {
    prompts_dir: PathBuf,
    crate_path: String,
}

impl PromptCodegen {
    /// Starts a builder for the prompt directory at `prompts_dir`, targeting
    /// the default crate path ([`DEFAULT_CRATE_PATH`]).
    #[must_use]
    pub fn new(prompts_dir: impl Into<PathBuf>) -> Self {
        Self {
            prompts_dir: prompts_dir.into(),
            crate_path: DEFAULT_CRATE_PATH.to_owned(),
        }
    }

    /// Sets the crate path generated code references core's types
    /// (`ToolSpec`, `ToolDefinition`, `NoParams`, the hidden `serde_json`
    /// re-export) through.
    #[must_use]
    pub fn crate_path(mut self, crate_path: impl Into<String>) -> Self {
        self.crate_path = crate_path.into();
        self
    }

    /// Loads, validates, and renders the generated module as a source string,
    /// without writing it.
    ///
    /// # Errors
    ///
    /// Returns a [`PromptError`] naming the offending file if loading or
    /// validating the prompt tree fails.
    pub fn generate(&self) -> Result<String, PromptError> {
        emit::generate(&self.prompts_dir, &self.crate_path)
    }

    /// Build-script entry: write the generated module to
    /// `$OUT_DIR/prompts.rs` and register rebuild-on-change. Call it from a
    /// consumer's `build.rs`; include the result with
    /// `include!(concat!(env!("OUT_DIR"), "/prompts.rs"))`.
    ///
    /// # Errors
    ///
    /// Returns a [`PromptError`] if `OUT_DIR` is unset (not run from a build
    /// script), if writing `prompts.rs` fails, or if loading/validating the
    /// tree fails.
    pub fn emit(&self) -> Result<(), PromptError> {
        let out_dir = std::env::var_os("OUT_DIR").ok_or_else(|| PromptError::Io {
            path: self.prompts_dir.clone(),
            message: "OUT_DIR is not set — emit must be called from a build script".to_owned(),
        })?;
        emit::emit_to(&self.prompts_dir, Path::new(&out_dir), &self.crate_path)
    }
}
