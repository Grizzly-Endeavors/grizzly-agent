//! The one dependency a Rust project adds to build an agent or otherwise work
//! with LLMs.
//!
//! This crate is a facade: it re-exports the runtime crates of the
//! `grizzly-agent` workspace behind features, so a consumer writes
//! `grizzly_agent::Message` rather than depending on each member crate by
//! name. A bare dependency with no features enabled gets
//! `grizzly-agent-core` alone — conversation types and the error taxonomy,
//! with no provider, skill, or eval machinery pulled in.
//!
//! Every re-export below is curated by name, never a glob, so two members'
//! same-named items can never silently collide or shadow each other.
//! Documentation for each item lives where it is defined, in its member
//! crate; this crate adds none of its own beyond this overview.

// Conversation types. Unconditional: core is a normal (non-optional)
// dependency, so these are present with no features enabled at all.
#[doc(inline)]
pub use grizzly_agent_core::Content;
#[doc(inline)]
pub use grizzly_agent_core::Message;
#[doc(inline)]
pub use grizzly_agent_core::Role;
#[doc(inline)]
pub use grizzly_agent_core::ToolResult;
#[doc(inline)]
pub use grizzly_agent_core::ToolUse;

// The error taxonomy. Unconditional for the same reason.
#[doc(inline)]
pub use grizzly_agent_core::ProviderFailure;
#[doc(inline)]
pub use grizzly_agent_core::ToolFailure;
#[doc(inline)]
pub use grizzly_agent_core::TurnFailure;
