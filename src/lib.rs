//! Foundational primitives for building LLM agents in Rust.
//!
//! This crate is a dependency, not an application. It provides the pieces an
//! agent is assembled from and leaves policy to the consumer.
//!
//! # Scope
//!
//! The boundary is deliberate: a primitive belongs here when two real projects
//! need it, it embeds no product decision, and neither project could define it
//! better locally. Anything encoding a product decision — which model, what a
//! tool may do, how a conversation is persisted — belongs in the consumer,
//! behind a trait this crate defines but does not implement.
//!
//! Some things are deliberately absent, each with a recorded reason in
//! `docs/decisions/`: MCP (use `rmcp` directly), streaming, and token counting
//! (providers report exact counts; this crate passes them through rather than
//! estimating).

mod error;
mod message;

pub use crate::error::{ProviderFailure, ToolFailure, TurnFailure};
pub use crate::message::{Content, Message, Role, ToolResult, ToolUse};
