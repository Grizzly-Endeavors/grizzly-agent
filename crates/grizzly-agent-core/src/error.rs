//! Failure types for the public API.
//!
//! Three of these exist for a reason worth stating: a tool that fails, a call
//! that fails, and a run that cannot be carried out are different events. A
//! tool failure is ordinary conversation — the model reads the message and
//! adapts — so [`ToolFailure`] converts into a [`crate::message::ToolResult`]
//! and the loop continues. A [`RunFailure`] means the loop cannot proceed:
//! the system broke, as distinct from the agent behaving in some way.
//! `job-finder` maintained this split by catching every exception at each
//! call site and remembering to; here the types enforce it.

use crate::agent::RunTrace;
use crate::message::ToolResult;

/// Why a call to a model provider failed.
///
/// Callers match on this to decide whether retrying can help, which is why it is
/// an enum rather than an opaque error. See [`ProviderFailure::is_retryable`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderFailure {
    /// The request never reached the provider, or the response never arrived.
    #[error("transport failure contacting {provider}")]
    Transport {
        /// Which provider was being called.
        provider: String,
        /// The underlying cause.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The provider answered with an error status.
    #[error("{provider} returned HTTP {status}: {message}")]
    Status {
        /// Which provider answered.
        provider: String,
        /// The HTTP status code.
        status: u16,
        /// The provider's error message, passed through unchanged.
        message: String,
        /// How long the provider asked us to wait, from `Retry-After` if present.
        retry_after: Option<std::time::Duration>,
    },

    /// The response arrived but could not be understood.
    ///
    /// Distinct from [`ProviderFailure::Status`] because retrying an identical
    /// request will produce the same unparseable response.
    #[error("could not decode {provider} response")]
    Decode {
        /// Which provider answered.
        provider: String,
        /// The underlying cause.
        #[source]
        source: serde_json::Error,
    },

    /// Credentials are missing or malformed, detected before any request.
    #[error("{0}")]
    Configuration(String),

    /// The request broke one of the request invariants `Model` enforces,
    /// caught before it reached a provider.
    ///
    /// Never retryable: the same request breaks the same invariant every time.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

impl ProviderFailure {
    /// Whether retrying the identical request could plausibly succeed.
    ///
    /// Classification is structural — transport failures, 408, 429, and 5xx —
    /// rather than substring-matched against the error message. `residuum` and
    /// `Ursix` both sniff for `"rate"`, `"429"`, and `"503"` in error text, which
    /// silently stops working when a provider rewords a message.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { .. } => true,
            Self::Status { status, .. } => {
                matches!(status, 408 | 429) || (500..600).contains(status)
            }
            Self::Decode { .. } | Self::Configuration(_) | Self::InvalidRequest(_) => false,
        }
    }

    /// How long the provider asked us to wait, if it said.
    ///
    /// A server's own `Retry-After` beats any backoff this crate would compute.
    #[must_use]
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            Self::Transport { .. }
            | Self::Decode { .. }
            | Self::Configuration(_)
            | Self::InvalidRequest(_) => None,
        }
    }
}

/// Why a tool did not produce a useful result.
///
/// This is not a loop-ending error. Every variant renders to text the model reads
/// and can act on, via [`ToolFailure::into_result`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ToolFailure {
    /// The model asked for a tool that is not registered.
    #[error("no tool named `{name}`")]
    Unknown {
        /// The name the model used.
        name: String,
        /// Registered names, so the message can suggest alternatives.
        available: Vec<String>,
    },

    /// The arguments did not match the tool's schema.
    ///
    /// The message is written to be read *by the model* as a correction, which is
    /// what makes a retry converge rather than repeat.
    #[error("invalid arguments for `{name}`: {reason}")]
    InvalidArguments {
        /// Which tool was called.
        name: String,
        /// What was wrong, phrased as an instruction.
        reason: String,
    },

    /// The tool ran and failed.
    #[error("`{name}` failed: {message}")]
    Execution {
        /// Which tool ran.
        name: String,
        /// What went wrong, in terms the model can act on.
        message: String,
    },
}

impl ToolFailure {
    /// Render this failure as the tool result the model will read.
    #[must_use]
    pub fn into_result(self, tool_use_id: impl Into<String>) -> ToolResult {
        ToolResult {
            tool_use_id: tool_use_id.into(),
            content: self.to_string(),
            is_error: true,
        }
    }
}

/// Why a run could not be carried out.
///
/// Running out of rounds and stalling are not run failures — they are run
/// endings the turn loop produces on [`crate::RunRecord`], describing what
/// the agent did rather than a breakage. Only an invalid conversation or a
/// provider failure means the system itself broke, which is why this is a
/// closed, two-variant enum rather than one open to future classification
/// like [`ProviderFailure`] and [`ToolFailure`].
#[derive(Debug, thiserror::Error)]
pub enum RunFailure {
    /// The conversation `Agent::run` was given broke a rule, caught before
    /// any work began.
    #[error("invalid conversation: {reason}")]
    InvalidConversation {
        /// The rule the conversation broke.
        reason: String,
    },

    /// A model call failed in a way retries could not recover from.
    ///
    /// Carries every round the run completed before the failure, so a
    /// caller that persists transcripts loses nothing by taking `trace` out
    /// of the error before propagating it.
    #[error("provider call failed")]
    Provider {
        /// The underlying provider failure. `RunFailure`'s [`std::error::Error::source`]
        /// is this, so the cause chain works with `?` and `anyhow`.
        #[source]
        source: ProviderFailure,
        /// Every round the run completed before the failure. Boxed to keep
        /// this variant from ballooning `RunFailure`'s size on every `Result`
        /// that returns it.
        trace: Box<RunTrace>,
    },
}

#[cfg(test)]
#[path = "tests/error.rs"]
mod tests;
