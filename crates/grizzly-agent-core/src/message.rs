//! Conversation types shared by every provider.
//!
//! Four projects in `docs/design/primitive-index.md` independently wrote a flat
//! `{ role, content: String, tool_calls, tool_call_id }` message — the OpenAI
//! wire shape. That shape cannot represent a turn that interleaves text and tool
//! use, which Anthropic's API produces natively, so it is a lossy target to
//! normalize toward. Content is a sequence of blocks here for that reason, and
//! the OpenAI adapter flattens on the way out rather than every caller flattening
//! on the way in.

use serde::{Deserialize, Serialize};

/// Who produced a message.
///
/// There is no `Tool` variant: a tool's output is a [`Content::ToolResult`]
/// block on a [`Role::User`] message, which is how the model actually sees it.
/// Providers that use a distinct tool role reconstruct it when serializing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Instructions that frame the conversation.
    System,
    /// Input from the caller, including tool results.
    User,
    /// Output from the model.
    Assistant,
}

/// One block within a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    /// Plain text.
    Text(String),
    /// The model's reasoning, kept out of [`Content::Text`] on purpose.
    ///
    /// Providers disagree on the field name — `thinking`, `reasoning`, and
    /// `reasoning_content` all appear in the wild, and `stan-eval-sidecar` hit
    /// this across two backends. Adapters map their spelling onto this variant so
    /// the ambiguity stops at the provider boundary. Reasoning is never folded
    /// into `Text`, because a caller rendering a reply must be able to leave it out.
    Reasoning {
        /// The reasoning text.
        text: String,
        /// An opaque, provider-issued signature over the reasoning.
        ///
        /// Some providers require prior reasoning to be sent back verbatim and
        /// signed before they will use it alongside a tool result; others use
        /// no signature at all, in which case this is `None`.
        signature: Option<String>,
    },
    /// The model asking for a tool to run.
    ToolUse(ToolUse),
    /// The result of running one.
    ToolResult(ToolResult),
}

/// A model's request to invoke a tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolUse {
    /// Provider-assigned id, echoed back on the matching [`ToolResult`].
    pub id: String,
    /// The tool's registered name.
    pub name: String,
    /// Arguments, shaped by the tool's schema. Not yet validated.
    pub input: serde_json::Value,
}

/// The outcome of running a tool, addressed back to the call that asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The [`ToolUse::id`] this answers.
    pub tool_use_id: String,
    /// What the model sees. A failure's message goes here, not into an error.
    pub content: String,
    /// Whether `content` describes a failure.
    ///
    /// A failed tool is ordinary conversation — the model reads the message and
    /// adapts. This flag lets a provider mark it as such where the wire format
    /// supports it, and lets a caller count failures without parsing prose.
    pub is_error: bool,
}

/// One turn in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Who produced it.
    pub role: Role,
    /// Its blocks, in order.
    pub content: Vec<Content>,
}

impl Message {
    /// A message with a single text block.
    #[must_use]
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![Content::Text(text.into())],
        }
    }

    /// A system message.
    #[must_use]
    pub fn system(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }

    /// A user message.
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }

    /// An assistant message.
    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }

    /// A user message carrying tool results, one block per result.
    #[must_use]
    pub fn tool_results(results: impl IntoIterator<Item = ToolResult>) -> Self {
        Self {
            role: Role::User,
            content: results.into_iter().map(Content::ToolResult).collect(),
        }
    }

    /// Every tool the model asked for in this message.
    #[must_use]
    pub fn tool_uses(&self) -> Vec<&ToolUse> {
        self.content
            .iter()
            .filter_map(|block| match block {
                Content::ToolUse(use_) => Some(use_),
                Content::Text(_) | Content::Reasoning { .. } | Content::ToolResult(_) => None,
            })
            .collect()
    }

    /// The message's text blocks joined by newlines, excluding reasoning.
    ///
    /// Returns an empty string for a message that is only tool use — which is a
    /// real case, not a defect, so it is not an `Option`.
    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                Content::Text(text) => Some(text.as_str()),
                Content::Reasoning { .. } | Content::ToolUse(_) | Content::ToolResult(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether this message asks for at least one tool.
    ///
    /// This is the turn loop's continue-or-stop signal: a model that asked for
    /// nothing has finished.
    #[must_use]
    pub fn requests_tools(&self) -> bool {
        self.content
            .iter()
            .any(|block| matches!(block, Content::ToolUse(_)))
    }
}

#[cfg(test)]
#[path = "tests/message.rs"]
mod tests;
