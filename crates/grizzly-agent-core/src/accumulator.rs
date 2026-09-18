//! Folding a [`CompletionEvent`] stream into a whole [`Completion`].

use std::collections::HashMap;

use crate::completion::{Completion, CompletionEvent, StopReason, Usage};
use crate::error::ProviderFailure;
use crate::message::{Content, ToolUse};

struct Finished {
    stop_reason: StopReason,
    raw_stop_reason: String,
    model: String,
}

/// Reassembles a stream of [`CompletionEvent`]s into a [`Completion`].
///
/// Public so a consumer rendering a stream live can keep the assembled result
/// without re-implementing reassembly: feed every event to [`Self::push`] as
/// it arrives, then call [`Self::finish`] once the stream ends.
#[derive(Default)]
pub struct CompletionAccumulator {
    blocks: Vec<Content>,
    tool_arguments: HashMap<String, String>,
    usage: Usage,
    finished: Option<Finished>,
}

impl CompletionAccumulator {
    /// An accumulator with nothing folded in yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one event in.
    ///
    /// A text or reasoning delta extends the current block of that kind, or
    /// starts a new one if the current block is a different kind — so block
    /// order in the reassembled message follows the stream.
    pub fn push(&mut self, event: CompletionEvent) {
        match event {
            CompletionEvent::TextDelta(text) => self.push_text(text),
            CompletionEvent::ReasoningDelta(text) => self.push_reasoning(text),
            CompletionEvent::ReasoningSignatureDelta(signature) => {
                self.push_reasoning_signature(&signature);
            }
            CompletionEvent::ToolUseStart { id, name } => self.push_tool_use_start(id, name),
            CompletionEvent::ToolUseArgumentsDelta { id, fragment } => {
                self.tool_arguments
                    .entry(id)
                    .or_default()
                    .push_str(&fragment);
            }
            CompletionEvent::Usage(usage) => self.usage = self.usage.merge(usage),
            CompletionEvent::Finished {
                stop_reason,
                raw_stop_reason,
                model,
            } => {
                self.finished = Some(Finished {
                    stop_reason,
                    raw_stop_reason,
                    model,
                });
            }
        }
    }

    fn push_text(&mut self, text: String) {
        if let Some(Content::Text(existing)) = self.blocks.last_mut() {
            existing.push_str(&text);
        } else {
            self.blocks.push(Content::Text(text));
        }
    }

    fn push_reasoning(&mut self, text: String) {
        if let Some(Content::Reasoning { text: existing, .. }) = self.blocks.last_mut() {
            existing.push_str(&text);
        } else {
            self.blocks.push(Content::Reasoning {
                text,
                signature: None,
            });
        }
    }

    fn push_reasoning_signature(&mut self, signature: &str) {
        if let Some(Content::Reasoning {
            signature: existing,
            ..
        }) = self.blocks.last_mut()
        {
            existing.get_or_insert_default().push_str(signature);
        }
    }

    fn push_tool_use_start(&mut self, id: String, name: String) {
        self.tool_arguments.entry(id.clone()).or_default();
        self.blocks.push(Content::ToolUse(ToolUse {
            id,
            name,
            input: serde_json::Value::Null,
        }));
    }

    /// Finalize the accumulated events into a [`Completion`], parsing each
    /// tool call's concatenated arguments as JSON.
    ///
    /// # Errors
    /// Returns a decode-class [`ProviderFailure`] when a tool call's
    /// arguments do not parse as JSON — except when the stop reason is
    /// [`StopReason::MaxTokens`], where a cut-off tool call is expected: that
    /// block is dropped and the rest of the completion is returned. Also
    /// returns a retryable transport-class failure if no
    /// [`CompletionEvent::Finished`] was ever pushed, since a stream that
    /// ends without one has failed.
    pub fn finish(self) -> Result<Completion, ProviderFailure> {
        let Some(finished) = self.finished else {
            return Err(unfinished_stream_failure());
        };

        let mut tool_arguments = self.tool_arguments;
        let mut content = Vec::with_capacity(self.blocks.len());
        for block in self.blocks {
            match block {
                Content::ToolUse(tool_use) => {
                    if let Some(resolved) = resolve_tool_use(
                        tool_use,
                        &mut tool_arguments,
                        finished.stop_reason,
                        &finished.model,
                    )? {
                        content.push(Content::ToolUse(resolved));
                    }
                }
                Content::Text(text) => content.push(Content::Text(text)),
                Content::Reasoning { text, signature } => {
                    content.push(Content::Reasoning { text, signature });
                }
                Content::ToolResult(result) => content.push(Content::ToolResult(result)),
            }
        }

        Ok(Completion {
            content,
            usage: self.usage,
            stop_reason: finished.stop_reason,
            raw_stop_reason: finished.raw_stop_reason,
            model: finished.model,
        })
    }
}

fn resolve_tool_use(
    mut tool_use: ToolUse,
    tool_arguments: &mut HashMap<String, String>,
    stop_reason: StopReason,
    model: &str,
) -> Result<Option<ToolUse>, ProviderFailure> {
    let raw = tool_arguments.remove(&tool_use.id).unwrap_or_default();
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(input) => {
            tool_use.input = input;
            Ok(Some(tool_use))
        }
        Err(source) => {
            if stop_reason == StopReason::MaxTokens {
                Ok(None)
            } else {
                Err(ProviderFailure::Decode {
                    provider: format!("{model}'s `{}` tool call", tool_use.name),
                    source,
                })
            }
        }
    }
}

fn unfinished_stream_failure() -> ProviderFailure {
    ProviderFailure::Transport {
        provider: "model provider".to_owned(),
        source: Box::new(std::io::Error::other(
            "completion stream ended without a finished event",
        )),
    }
}

#[cfg(test)]
#[path = "tests/accumulator.rs"]
mod tests;
