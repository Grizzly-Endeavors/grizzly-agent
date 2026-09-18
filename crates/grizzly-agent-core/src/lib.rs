//! Conversation types and the error taxonomy — the runtime core of
//! `grizzly-agent`.
//!
//! This crate has no HTTP dependency and no I/O. It defines the shapes and
//! contracts that `grizzly-agent-providers`, `grizzly-agent-skills`, and
//! `grizzly-agent-eval` build on, and that the `grizzly-agent` facade
//! re-exports for consumers who add only one dependency line.
//!
//! Depending on this crate directly, instead of through the facade, is only
//! useful when a consumer wants core alone with no facade indirection — for
//! example a build script generating code that references its types.

mod accumulator;
mod agent;
mod completion;
mod error;
mod message;
mod model;
mod provider;
mod request;
mod retry;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
mod tools;

pub use crate::accumulator::CompletionAccumulator;
pub use crate::agent::{
    Agent, AgentBuilder, DynamicSection, Limits, RoundRecord, RunEnding, RunObserver, RunRecord,
    RunTrace, SystemSection, ToolCallRecord,
};
pub use crate::completion::{Completion, CompletionEvent, StopReason, Usage};
pub use crate::error::{ProviderFailure, RunFailure, ToolFailure};
pub use crate::message::{Content, Message, Role, ToolResult, ToolUse};
pub use crate::model::{Model, ModelBuilder};
pub use crate::provider::{CompletionStream, Provider};
pub use crate::request::{CompletionRequest, ResponseFormat};
pub use crate::retry::RetryPolicy;
#[cfg(any(test, feature = "test-support"))]
pub use crate::test_support::{ScriptedProvider, ScriptedResponse};
pub use crate::tools::{
    DuplicateToolName, NoParams, StopRequest, ToolContext, ToolDefinition, ToolHandler, ToolSet,
    ToolSpec, TypedToolHandler,
};

/// Re-exported so generated prompt code builds [`ToolSpec::parameters`]
/// against this crate's `serde_json`, not whatever version a consumer happens
/// to depend on separately. Not part of the public API; the leading
/// underscore marks it as codegen support that may change without a semver
/// bump.
#[doc(hidden)]
pub use serde_json;
