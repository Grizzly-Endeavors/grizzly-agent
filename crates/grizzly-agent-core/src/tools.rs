//! The tool model: the advertisement sent to the model
//! ([`ToolSpec`]/[`ToolDefinition`]/[`NoParams`]), the object-safe handler
//! every tool implements ([`ToolHandler`]) and the typed adapter that builds
//! one from a plain async function ([`TypedToolHandler`]), the per-run side
//! channel between tools and the turn loop ([`ToolContext`]/[`StopRequest`]),
//! and the registry the loop advertises and dispatches through ([`ToolSet`]).

mod context;
mod handler;
mod set;
mod spec;

pub use context::{StopRequest, ToolContext};
pub use handler::{ToolHandler, TypedToolHandler};
pub use set::{DuplicateToolName, ToolSet};
pub use spec::{NoParams, ToolDefinition, ToolSpec};
