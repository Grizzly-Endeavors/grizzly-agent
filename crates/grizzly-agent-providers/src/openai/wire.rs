//! Pure translation between core's canonical types and the OpenAI-compatible
//! chat-completions wire shape, in both directions. No I/O.

use std::collections::HashMap;

use grizzly_agent_core::{
    CompletionEvent, CompletionRequest, Content, Message, ResponseFormat, Role, StopReason,
    ToolResult, ToolSpec,
};
use serde::{Deserialize, Serialize};

// --- Outgoing: `CompletionRequest` to the wire ------------------------------

/// The outgoing request body.
#[derive(Debug, Serialize)]
pub(crate) struct RequestWire<'a> {
    pub(crate) model: &'a str,
    pub(crate) stream: bool,
    pub(crate) stream_options: StreamOptionsWire,
    pub(crate) messages: Vec<MessageWire>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools: Vec<ToolWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) response_format: Option<ResponseFormatWire>,
}

/// Streaming knobs: usage totals only ride a streamed response when asked
/// for, and every call streams.
#[derive(Debug, Serialize)]
pub(crate) struct StreamOptionsWire {
    pub(crate) include_usage: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct MessageWire {
    role: WireRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ToolCallWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum WireRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Serialize)]
struct ToolCallWire {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    function: FunctionCallWire,
}

#[derive(Debug, Serialize)]
struct FunctionCallWire {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolWire {
    #[serde(rename = "type")]
    kind: &'static str,
    function: FunctionDefWire,
}

#[derive(Debug, Serialize)]
pub(crate) struct FunctionDefWire {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

/// A `response_format` block requesting strict schema-constrained JSON.
#[derive(Debug, Serialize)]
pub(crate) struct ResponseFormatWire {
    #[serde(rename = "type")]
    kind: &'static str,
    json_schema: JsonSchemaWire,
}

#[derive(Debug, Serialize)]
pub(crate) struct JsonSchemaWire {
    name: String,
    strict: bool,
    schema: serde_json::Value,
}

/// Build the outgoing request body for `request`, targeting `model`.
///
/// System messages pass through as ordinary `system`-role wire messages —
/// unlike Anthropic, the chat-completions wire has no separate system field,
/// so [`crate::AnthropicProvider`]'s head-lifting rule does not apply here.
/// A user message's tool results become consecutive `tool`-role messages,
/// followed by one `user`-role message holding its remaining text, if any.
/// Reasoning blocks are dropped from outgoing history: this wire has no slot
/// for them, and the chat-completions API never asks for them back.
pub(crate) fn build_request<'a>(request: &CompletionRequest, model: &'a str) -> RequestWire<'a> {
    RequestWire {
        model,
        stream: true,
        stream_options: StreamOptionsWire {
            include_usage: true,
        },
        messages: request.messages.iter().flat_map(message_to_wire).collect(),
        tools: request.tools.iter().map(tool_to_wire).collect(),
        max_tokens: request.max_tokens,
        temperature: request.temperature,
        response_format: request
            .response_format
            .as_ref()
            .map(response_format_to_wire),
    }
}

fn message_to_wire(message: &Message) -> Vec<MessageWire> {
    match message.role {
        Role::System => vec![MessageWire {
            role: WireRole::System,
            content: Some(message.text_content()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }],
        Role::User => user_message_to_wire(message),
        Role::Assistant => vec![assistant_message_to_wire(message)],
    }
}

fn user_message_to_wire(message: &Message) -> Vec<MessageWire> {
    let mut wire = Vec::new();
    for block in &message.content {
        if let Content::ToolResult(result) = block {
            wire.push(tool_result_to_wire(result));
        }
    }
    let text = message.text_content();
    if !text.is_empty() {
        wire.push(MessageWire {
            role: WireRole::User,
            content: Some(text),
            tool_calls: Vec::new(),
            tool_call_id: None,
        });
    }
    wire
}

fn tool_result_to_wire(result: &ToolResult) -> MessageWire {
    MessageWire {
        role: WireRole::Tool,
        content: Some(result.content.clone()),
        tool_calls: Vec::new(),
        tool_call_id: Some(result.tool_use_id.clone()),
    }
}

fn assistant_message_to_wire(message: &Message) -> MessageWire {
    let text = message.text_content();
    let tool_calls = message
        .tool_uses()
        .into_iter()
        .map(|tool_use| ToolCallWire {
            id: tool_use.id.clone(),
            kind: "function",
            function: FunctionCallWire {
                name: tool_use.name.clone(),
                arguments: tool_use.input.to_string(),
            },
        })
        .collect();
    MessageWire {
        role: WireRole::Assistant,
        content: (!text.is_empty()).then_some(text),
        tool_calls,
        tool_call_id: None,
    }
}

fn tool_to_wire(tool: &ToolSpec) -> ToolWire {
    ToolWire {
        kind: "function",
        function: FunctionDefWire {
            name: tool.name.clone().into_owned(),
            description: tool.description.clone().into_owned(),
            parameters: tool.parameters.clone(),
        },
    }
}

fn response_format_to_wire(format: &ResponseFormat) -> ResponseFormatWire {
    ResponseFormatWire {
        kind: "json_schema",
        json_schema: JsonSchemaWire {
            name: format.name.clone(),
            strict: true,
            schema: format.schema.clone(),
        },
    }
}

// --- Incoming: a streamed chunk to `CompletionEvent`s -----------------------

/// One chunk of a streamed completion, as served.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ChunkWire {
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    pub(crate) choices: Vec<ChoiceWire>,
    #[serde(default)]
    pub(crate) usage: Option<UsageWire>,
    /// Some stacks report a mid-stream failure as a data payload rather than
    /// an HTTP status; without this field it would read as an empty chunk
    /// and the failure would be silent.
    #[serde(default)]
    pub(crate) error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ChoiceWire {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    delta: DeltaWire,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DeltaWire {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDeltaWire>,
}

impl DeltaWire {
    /// The reasoning fragment under whichever spelling this stack uses.
    /// `reasoning_content`, `reasoning`, and `thinking` all appear across
    /// real OpenAI-compatible stacks for the same concept; the first present
    /// wins.
    fn reasoning_text(&self) -> Option<&str> {
        self.reasoning_content
            .as_deref()
            .or(self.reasoning.as_deref())
            .or(self.thinking.as_deref())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ToolCallDeltaWire {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDeltaWire>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct FunctionDeltaWire {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct UsageWire {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

impl From<UsageWire> for grizzly_agent_core::Usage {
    fn from(usage: UsageWire) -> Self {
        Self {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
        }
    }
}

fn classify_stop_reason(raw: &str) -> StopReason {
    match raw {
        "stop" => StopReason::EndOfTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        _ => StopReason::Other,
    }
}

struct PendingFinish {
    stop_reason: StopReason,
    raw_stop_reason: String,
}

/// Folds a sequence of [`ChunkWire`]s into [`CompletionEvent`]s.
///
/// Stateful because two things a single chunk cannot express on its own
/// require it: reassembling indexed tool-call argument fragments into
/// id-addressed events, and holding the finish reason until the stream truly
/// ends — chat-completions stacks that request usage in the stream send a
/// trailing usage-only chunk *after* the chunk carrying `finish_reason`, and
/// [`CompletionEvent::Finished`] must be the stream's last event.
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

    /// Fold one chunk in, returning the events it produces immediately.
    /// A chunk carrying `finish_reason` is buffered rather than translated
    /// into [`CompletionEvent::Finished`] right away; call [`Self::finalize`]
    /// once the stream ends to release it.
    pub(crate) fn absorb(&mut self, chunk: ChunkWire) -> Vec<CompletionEvent> {
        if let Some(model) = chunk.model {
            self.model = Some(model);
        }
        let mut events = Vec::new();
        if let Some(usage) = chunk.usage {
            events.push(CompletionEvent::Usage(usage.into()));
        }
        for choice in chunk.choices.into_iter().filter(|choice| choice.index == 0) {
            self.absorb_choice(choice, &mut events);
        }
        events
    }

    fn absorb_choice(&mut self, choice: ChoiceWire, events: &mut Vec<CompletionEvent>) {
        if let Some(text) = choice.delta.reasoning_text() {
            events.push(CompletionEvent::ReasoningDelta(text.to_owned()));
        }
        if let Some(text) = choice.delta.content {
            events.push(CompletionEvent::TextDelta(text));
        }
        for delta in choice.delta.tool_calls {
            self.absorb_tool_call_delta(delta, events);
        }
        if let Some(raw) = choice.finish_reason {
            self.pending_finish = Some(PendingFinish {
                stop_reason: classify_stop_reason(&raw),
                raw_stop_reason: raw,
            });
        }
    }

    fn absorb_tool_call_delta(
        &mut self,
        delta: ToolCallDeltaWire,
        events: &mut Vec<CompletionEvent>,
    ) {
        let is_new = !self.tool_call_ids.contains_key(&delta.index);
        if is_new {
            let id = delta
                .id
                .clone()
                .unwrap_or_else(|| format!("tool_call_{}", delta.index));
            self.tool_call_ids.insert(delta.index, id.clone());
            let name = delta
                .function
                .as_ref()
                .and_then(|function| function.name.clone())
                .unwrap_or_default();
            events.push(CompletionEvent::ToolUseStart { id, name });
        }
        let Some(fragment) = delta.function.and_then(|function| function.arguments) else {
            return;
        };
        // `absorb_tool_call_delta` always inserts `delta.index` above before
        // reaching here, on this call or an earlier one.
        if let Some(id) = self.tool_call_ids.get(&delta.index) {
            events.push(CompletionEvent::ToolUseArgumentsDelta {
                id: id.clone(),
                fragment,
            });
        }
    }

    /// Release the buffered finish event once the stream has truly ended.
    /// `None` means the stream ended without ever seeing `finish_reason` —
    /// the caller's cue to fail retryably instead.
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
