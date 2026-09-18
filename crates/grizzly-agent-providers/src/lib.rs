//! Concrete [`grizzly_agent_core::Provider`] clients.
//!
//! Each provider lives behind its own feature (`openai`, `anthropic`) so a
//! consumer building against one endpoint never compiles or links an HTTP
//! client for the other. Both providers always stream on the wire — every
//! provider API here streams regardless of what the caller asked for — and
//! translate the wire format into [`grizzly_agent_core::CompletionEvent`]s
//! through pure, I/O-free functions that are tested without a network. The
//! byte-level line buffering and status/`Retry-After` classification the two
//! providers share live in an internal transport module neither exposes.

#[cfg(any(feature = "openai", feature = "anthropic"))]
mod transport;

#[cfg(feature = "openai")]
mod openai;
#[cfg(feature = "openai")]
pub use openai::{OpenAiCompatibleProvider, OpenAiCompatibleProviderBuilder};

#[cfg(feature = "anthropic")]
mod anthropic;
#[cfg(feature = "anthropic")]
pub use anthropic::{AnthropicProvider, AnthropicProviderBuilder};
