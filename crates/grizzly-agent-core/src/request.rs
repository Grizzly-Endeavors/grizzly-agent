//! What goes into a model call: the request shape and the invariants
//! [`crate::Model`] enforces on it before any provider sees it.

use std::collections::HashSet;

use crate::error::ProviderFailure;
use crate::message::{Content, Message, Role};
use crate::tools::ToolSpec;

/// A request for schema-constrained JSON output.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseFormat {
    /// A name for the schema, as the provider's structured-output mechanism
    /// requires one.
    pub name: String,
    /// The JSON Schema the reply must conform to.
    pub schema: serde_json::Value,
}

/// A request to complete a conversation.
///
/// Unset `max_tokens` and `temperature` fall through to [`crate::Model`]'s
/// configured defaults, then to the provider's own defaults.
#[derive(Debug, Clone, Default)]
pub struct CompletionRequest {
    /// The conversation so far, oldest first.
    pub messages: Vec<Message>,
    /// The tools advertised to the model for this request.
    pub tools: Vec<ToolSpec>,
    /// The maximum number of output tokens, if bounded.
    pub max_tokens: Option<u32>,
    /// Sampling temperature, if set.
    pub temperature: Option<f32>,
    /// A schema-constrained JSON output format, if requested.
    pub response_format: Option<ResponseFormat>,
}

impl CompletionRequest {
    /// A request with only its conversation set; every other field falls
    /// through to defaults.
    #[must_use]
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            messages,
            ..Self::default()
        }
    }

    /// Check the request invariants [`crate::Model`] promises providers:
    ///
    /// - system messages form a contiguous, text-only run at the head;
    /// - tool-use and reasoning blocks appear only on assistant messages;
    /// - tool-result blocks appear only on user messages;
    /// - every tool result answers a tool use from an earlier assistant
    ///   message.
    ///
    /// # Errors
    /// Returns [`ProviderFailure::InvalidRequest`] naming the rule the first
    /// violation breaks.
    pub(crate) fn validate(&self) -> Result<(), ProviderFailure> {
        let mut past_system_head = false;
        let mut known_tool_use_ids = HashSet::new();

        for message in &self.messages {
            match message.role {
                Role::System => {
                    validate_system_message(message, past_system_head)?;
                }
                Role::User => {
                    past_system_head = true;
                    validate_user_message(message, &known_tool_use_ids)?;
                }
                Role::Assistant => {
                    past_system_head = true;
                    validate_assistant_message(message, &mut known_tool_use_ids)?;
                }
            }
        }

        Ok(())
    }
}

fn invalid(rule: &str) -> ProviderFailure {
    ProviderFailure::InvalidRequest(rule.to_owned())
}

fn validate_system_message(
    message: &Message,
    past_system_head: bool,
) -> Result<(), ProviderFailure> {
    if past_system_head {
        return Err(invalid(
            "system messages must appear only as a contiguous run at the head of the conversation",
        ));
    }
    let text_only = message
        .content
        .iter()
        .all(|block| matches!(block, Content::Text(_)));
    if !text_only {
        return Err(invalid("system messages must contain only text blocks"));
    }
    Ok(())
}

fn validate_user_message(
    message: &Message,
    known_tool_use_ids: &HashSet<String>,
) -> Result<(), ProviderFailure> {
    for block in &message.content {
        match block {
            Content::Text(_) => {}
            Content::ToolUse(_) | Content::Reasoning { .. } => {
                return Err(invalid(
                    "tool-use and reasoning blocks may appear only on assistant messages",
                ));
            }
            Content::ToolResult(result) => {
                if !known_tool_use_ids.contains(&result.tool_use_id) {
                    return Err(invalid(
                        "every tool result must answer a tool use from an earlier assistant message",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_assistant_message(
    message: &Message,
    known_tool_use_ids: &mut HashSet<String>,
) -> Result<(), ProviderFailure> {
    for block in &message.content {
        match block {
            Content::Text(_) | Content::Reasoning { .. } => {}
            Content::ToolUse(tool_use) => {
                known_tool_use_ids.insert(tool_use.id.clone());
            }
            Content::ToolResult(_) => {
                return Err(invalid(
                    "tool-result blocks may appear only on user messages",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/request.rs"]
mod tests;
