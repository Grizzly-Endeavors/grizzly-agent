//! Pure translation between core's canonical types and the Anthropic
//! Messages wire shape, in both directions. No I/O.
//!
//! `output_config.format` (native structured-output) verified against
//! `platform.claude.com/docs/en/build-with-claude/structured-outputs` and
//! `platform.claude.com/docs/en/api/messages/create`: current, generally
//! available, no beta header required. The prior `output_format` field and
//! `structured-outputs-*` beta header are accepted for compatibility by the
//! API but not emitted here.

use std::collections::HashMap;

use grizzly_agent_core::{
    CompletionEvent, CompletionRequest, Content, Message, ResponseFormat, Role, StopReason,
    ToolResult, ToolSpec, ToolUse,
};
use serde::{Deserialize, Serialize};

// --- Outgoing: `CompletionRequest` to the wire ------------------------------

/// The outgoing request body.
#[derive(Debug, Serialize)]
pub(crate) struct RequestWire<'a> {
    pub(crate) model: &'a str,
    pub(crate) stream: bool,
    pub(crate) max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system: Option<String>,
    pub(crate) messages: Vec<MessageWire>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools: Vec<ToolWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_config: Option<OutputConfigWire>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageWire {
    role: WireRole,
    content: Vec<BlockWire>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum WireRole {
    User,
    Assistant,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum BlockWire {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolWire {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub(crate) struct OutputConfigWire {
    format: OutputFormatWire,
}

#[derive(Debug, Serialize)]
pub(crate) struct OutputFormatWire {
    #[serde(rename = "type")]
    kind: &'static str,
    schema: serde_json::Value,
}

/// Build the outgoing request body for `request`, targeting `model`, with
/// `max_tokens` falling back to `default_max_tokens` when `request` sets
/// none — Anthropic requires the field on every call, unlike the
/// OpenAI-compatible wire, where it is optional.
///
/// Head system messages are lifted and joined with a blank line into the
/// top-level `system` field. Content blocks map natively in both message
/// roles. Reasoning is sent back only when it carries a signature — unsigned
/// reasoning is dropped, per the request invariants `Model` already enforces
/// (reasoning appears only on assistant messages).
pub(crate) fn build_request<'a>(
    request: &CompletionRequest,
    model: &'a str,
    default_max_tokens: u32,
) -> RequestWire<'a> {
    let system = system_prompt(&request.messages);
    let messages = request
        .messages
        .iter()
        .filter(|message| message.role != Role::System)
        .map(message_to_wire)
        .collect();
    RequestWire {
        model,
        stream: true,
        max_tokens: request.max_tokens.unwrap_or(default_max_tokens),
        system,
        messages,
        tools: request.tools.iter().map(tool_to_wire).collect(),
        temperature: request.temperature,
        output_config: request
            .response_format
            .as_ref()
            .map(response_format_to_wire),
    }
}

fn system_prompt(messages: &[Message]) -> Option<String> {
    let joined = messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .map(Message::text_content)
        .collect::<Vec<_>>()
        .join("\n\n");
    (!joined.is_empty()).then_some(joined)
}

fn message_to_wire(message: &Message) -> MessageWire {
    let role = match message.role {
        Role::User => WireRole::User,
        Role::Assistant | Role::System => WireRole::Assistant,
    };
    let content = message.content.iter().filter_map(block_to_wire).collect();
    MessageWire { role, content }
}

fn block_to_wire(block: &Content) -> Option<BlockWire> {
    match block {
        Content::Text(text) => Some(BlockWire::Text { text: text.clone() }),
        Content::ToolUse(tool_use) => Some(tool_use_to_wire(tool_use)),
        Content::ToolResult(result) => Some(tool_result_to_wire(result)),
        Content::Reasoning { text, signature } => {
            let signature = signature.clone()?;
            (!text.is_empty()).then_some(BlockWire::Thinking {
                thinking: text.clone(),
                signature,
            })
        }
    }
}

fn tool_use_to_wire(tool_use: &ToolUse) -> BlockWire {
    BlockWire::ToolUse {
        id: tool_use.id.clone(),
        name: tool_use.name.clone(),
        input: tool_use.input.clone(),
    }
}

fn tool_result_to_wire(result: &ToolResult) -> BlockWire {
    BlockWire::ToolResult {
        tool_use_id: result.tool_use_id.clone(),
        content: result.content.clone(),
        is_error: result.is_error,
    }
}

fn tool_to_wire(tool: &ToolSpec) -> ToolWire {
    ToolWire {
        name: tool.name.clone().into_owned(),
        description: tool.description.clone().into_owned(),
        input_schema: tool.parameters.clone(),
    }
}

fn response_format_to_wire(format: &ResponseFormat) -> OutputConfigWire {
    OutputConfigWire {
        format: OutputFormatWire {
            kind: "json_schema",
            schema: format.schema.clone(),
        },
    }
}

// --- Incoming: a streamed event to `CompletionEvent`s -----------------------

/// One Messages API streaming event, discriminated by its `type` field.
/// Event and block kinds this crate does not translate (server tool use,
/// citations, and future additions) fall through to their `Other` variant
/// rather than failing to parse.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum StreamEventWire {
    MessageStart {
        message: MessageStartWire,
    },
    ContentBlockStart {
        index: u32,
        content_block: ContentBlockWire,
    },
    ContentBlockDelta {
        index: u32,
        delta: ContentDeltaWire,
    },
    ContentBlockStop {},
    MessageDelta {
        delta: MessageDeltaWire,
        usage: MessageDeltaUsageWire,
    },
    MessageStop {},
    Ping {},
    Error {
        error: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MessageStartWire {
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) usage: Option<UsageWire>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ContentBlockWire {
    ToolUse {
        id: String,
        name: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ContentDeltaWire {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    SignatureDelta {
        signature: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MessageDeltaWire {
    #[serde(default)]
    pub(crate) stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MessageDeltaUsageWire {
    #[serde(default)]
    pub(crate) input_tokens: Option<u64>,
    #[serde(default)]
    pub(crate) output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct UsageWire {
    #[serde(default)]
    pub(crate) input_tokens: Option<u64>,
    #[serde(default)]
    pub(crate) output_tokens: Option<u64>,
}

impl From<UsageWire> for grizzly_agent_core::Usage {
    fn from(usage: UsageWire) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }
    }
}

impl From<MessageDeltaUsageWire> for grizzly_agent_core::Usage {
    fn from(usage: MessageDeltaUsageWire) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        }
    }
}

fn classify_stop_reason(raw: &str) -> StopReason {
    match raw {
        "end_turn" => StopReason::EndOfTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

struct PendingFinish {
    stop_reason: StopReason,
    raw_stop_reason: String,
}

/// Folds a sequence of [`StreamEventWire`]s into [`CompletionEvent`]s.
///
/// Stateful for the same two reasons as the OpenAI-compatible translator:
/// `input_json_delta` fragments are addressed by content-block index, not
/// id, so the id `content_block_start` reports must be remembered; and the
/// finish reason (on `message_delta`) arrives before the stream's true
/// terminator (`message_stop`), so it is held until then.
#[derive(Default)]
pub(crate) struct EventTranslator {
    tool_call_ids: HashMap<u32, String>,
    model: Option<String>,
    pending_finish: Option<PendingFinish>,
}

impl EventTranslator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Fold one event in. `Ok` carries the events it produced immediately —
    /// possibly none, for a `ping` or a block-stop. `Err` means the event
    /// was a mid-stream error payload.
    pub(crate) fn absorb(
        &mut self,
        event: StreamEventWire,
    ) -> Result<Vec<CompletionEvent>, serde_json::Value> {
        match event {
            StreamEventWire::MessageStart { message } => {
                self.model = message.model;
                Ok(message
                    .usage
                    .map(|usage| vec![CompletionEvent::Usage(usage.into())])
                    .unwrap_or_default())
            }
            StreamEventWire::ContentBlockStart {
                index,
                content_block: ContentBlockWire::ToolUse { id, name },
            } => {
                self.tool_call_ids.insert(index, id.clone());
                Ok(vec![CompletionEvent::ToolUseStart { id, name }])
            }
            StreamEventWire::ContentBlockDelta { index, delta } => {
                Ok(self.absorb_delta(index, delta))
            }
            StreamEventWire::MessageDelta { delta, usage } => {
                self.pending_finish = Some(PendingFinish {
                    stop_reason: delta
                        .stop_reason
                        .as_deref()
                        .map_or(StopReason::Other, classify_stop_reason),
                    raw_stop_reason: delta.stop_reason.unwrap_or_default(),
                });
                Ok(vec![CompletionEvent::Usage(usage.into())])
            }
            // A block-level start/stop we don't track further, `message_stop`
            // (the caller releases the buffered finish event once it sees
            // this event itself, via `finalize`), a keepalive `ping`, or a
            // block/event kind this crate does not translate: none produce
            // an event here.
            StreamEventWire::ContentBlockStart { .. }
            | StreamEventWire::ContentBlockStop {}
            | StreamEventWire::MessageStop {}
            | StreamEventWire::Ping {}
            | StreamEventWire::Other => Ok(Vec::new()),
            StreamEventWire::Error { error } => Err(error),
        }
    }

    fn absorb_delta(&mut self, index: u32, delta: ContentDeltaWire) -> Vec<CompletionEvent> {
        match delta {
            ContentDeltaWire::TextDelta { text } => vec![CompletionEvent::TextDelta(text)],
            ContentDeltaWire::ThinkingDelta { thinking } => {
                vec![CompletionEvent::ReasoningDelta(thinking)]
            }
            ContentDeltaWire::SignatureDelta { signature } => {
                vec![CompletionEvent::ReasoningSignatureDelta(signature)]
            }
            ContentDeltaWire::InputJsonDelta { partial_json } => self
                .tool_call_ids
                .get(&index)
                .map(|id| {
                    vec![CompletionEvent::ToolUseArgumentsDelta {
                        id: id.clone(),
                        fragment: partial_json,
                    }]
                })
                .unwrap_or_default(),
            ContentDeltaWire::Other => Vec::new(),
        }
    }

    /// Whether `message_stop` (the stream's true terminator) has a finish
    /// reason to release. `Some` when [`Self::finalize`] would produce a
    /// [`CompletionEvent::Finished`]; `None` means the stream ended without
    /// ever seeing one, the caller's cue to fail retryably instead.
    pub(crate) fn finalize(self) -> Option<CompletionEvent> {
        let finish = self.pending_finish?;
        Some(CompletionEvent::Finished {
            stop_reason: finish.stop_reason,
            raw_stop_reason: finish.raw_stop_reason,
            model: self.model.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
#[path = "tests/wire.rs"]
mod tests;
