//! What a model call returns: streamed events, usage, stop reasons, and the
//! whole [`Completion`] they fold into.

/// One step of a streamed completion, in provider-neutral terms.
///
/// Closed rather than [`non_exhaustive`](https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute):
/// every provider translates its wire format into exactly this vocabulary, so a
/// caller matching on it exhaustively is never surprised by a new variant.
#[derive(Debug, Clone, PartialEq)]
pub enum CompletionEvent {
    /// A fragment of text, appended to the current text block.
    ///
    /// A delta that follows a block of a different kind starts a new text
    /// block, so block order in the reassembled message follows the stream.
    TextDelta(String),
    /// A fragment of reasoning, appended to the current reasoning block.
    ///
    /// Never folded into [`CompletionEvent::TextDelta`]: reasoning and the
    /// visible reply are distinct blocks throughout this crate.
    ReasoningDelta(String),
    /// The start of a tool call: its id and the tool's name.
    ///
    /// Opens a new tool-use block. Its arguments arrive afterward as
    /// [`CompletionEvent::ToolUseArgumentsDelta`] fragments addressed by `id`.
    ToolUseStart {
        /// The provider-assigned id, echoed on `ToolUseArgumentsDelta`.
        id: String,
        /// The tool's registered name.
        name: String,
    },
    /// A fragment of one tool call's JSON arguments.
    ///
    /// Fragments for the same `id` concatenate in arrival order; the result is
    /// parsed as JSON once the stream finishes.
    ToolUseArgumentsDelta {
        /// Which tool call this fragment belongs to.
        id: String,
        /// The next slice of the arguments' JSON text.
        fragment: String,
    },
    /// Usage the provider reported.
    ///
    /// May arrive more than once in a stream; later values supersede earlier
    /// ones field by field, so a field a later event leaves unset keeps
    /// whatever value an earlier event reported.
    Usage(Usage),
    /// The stream's terminal event: always the last event of a successful
    /// stream.
    Finished {
        /// The stop reason, classified into [`StopReason`]'s closed set.
        stop_reason: StopReason,
        /// The provider's own stop-reason string, unclassified.
        raw_stop_reason: String,
        /// The model identifier the provider reports for this completion.
        model: String,
    },
}

/// Token usage for a completion.
///
/// Every field is independently optional: a provider that never reports a
/// count leaves it `None` — *unknown*, never a silent zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Tokens in the request the model read.
    pub input_tokens: Option<u64>,
    /// Tokens the model generated.
    pub output_tokens: Option<u64>,
}

impl Usage {
    /// Merge `update` over `self`, field by field: a field `update` reports
    /// (`Some`) replaces `self`'s; a field it leaves unset keeps `self`'s.
    #[must_use]
    pub fn merge(self, update: Self) -> Self {
        Self {
            input_tokens: update.input_tokens.or(self.input_tokens),
            output_tokens: update.output_tokens.or(self.output_tokens),
        }
    }
}

/// Why a completion stopped, classified into a small closed set.
///
/// Paired with the provider's own raw string on [`Completion`] and
/// [`CompletionEvent::Finished`], so a caller gets both the portable
/// classification and the original wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished its reply without requesting a tool.
    EndOfTurn,
    /// The model's reply requests at least one tool.
    ToolUse,
    /// The model hit its output-token limit before finishing.
    MaxTokens,
    /// A stop reason this crate does not classify further (content
    /// filtering, an explicit stop sequence, and similar provider-specific
    /// reasons all land here); the raw string carries the detail.
    Other,
}

/// A whole completion, assembled from a stream by [`crate::CompletionAccumulator`].
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    /// The assistant's reply, as content blocks in stream order.
    pub content: Vec<crate::message::Content>,
    /// Usage reported for this completion. Every field unknown if the
    /// provider reported none.
    pub usage: Usage,
    /// The stop reason, classified.
    pub stop_reason: StopReason,
    /// The provider's own stop-reason string, unclassified.
    pub raw_stop_reason: String,
    /// The model identifier the provider reported.
    pub model: String,
}

#[cfg(test)]
#[path = "tests/completion.rs"]
mod tests;
