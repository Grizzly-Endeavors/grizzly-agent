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

mod error;
mod message;
mod tools;

pub use crate::error::{ProviderFailure, ToolFailure, TurnFailure};
pub use crate::message::{Content, Message, Role, ToolResult, ToolUse};
pub use crate::tools::{
    DuplicateToolName, NoParams, StopRequest, ToolContext, ToolDefinition, ToolHandler, ToolSet,
    ToolSpec, TypedToolHandler,
};
