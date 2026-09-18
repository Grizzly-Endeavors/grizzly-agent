//! Drives a streaming HTTP response through [`super::wire`]'s translation
//! into a [`CompletionStream`].
//!
//! Split the same way as the OpenAI-compatible provider's reader: a pure,
//! synchronous [`ChunkedReader`] that reassembles [`CompletionEvent`]s from
//! transport bytes, tested with no socket, and a thin async wrapper that
//! feeds it what a `reqwest::Response` returns.

use std::collections::VecDeque;

use futures_util::stream::{self, StreamExt};
use grizzly_agent_core::{CompletionEvent, CompletionStream, ProviderFailure};

use super::wire::{EventTranslator, StreamEventWire};
use crate::transport::{LineBuffer, transport_failure};

type Item = Result<CompletionEvent, ProviderFailure>;

/// Turns transport bytes into a queue of [`CompletionEvent`]s and failures,
/// with no I/O of its own.
pub(crate) struct ChunkedReader {
    provider: String,
    lines: LineBuffer,
    translator: EventTranslator,
    pending: VecDeque<Item>,
    /// Set once nothing further will ever be queued: `message_stop`, a
    /// clean transport close, or a failure all end the reader the same way.
    done: bool,
}

impl ChunkedReader {
    pub(crate) fn new(provider: String) -> Self {
        Self {
            provider,
            lines: LineBuffer::default(),
            translator: EventTranslator::new(),
            pending: VecDeque::new(),
            done: false,
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done && self.pending.is_empty()
    }

    pub(crate) fn pop(&mut self) -> Option<Item> {
        self.pending.pop_front()
    }

    /// Feed one transport read in.
    pub(crate) fn absorb(&mut self, bytes: &[u8]) {
        if self.done {
            return;
        }
        let lines = self.lines.push(bytes);
        self.absorb_lines(lines);
    }

    /// The transport closed cleanly: flush any unterminated trailing line,
    /// then release the buffered finish event, or fail retryably when the
    /// stream ended without ever seeing `message_stop`.
    pub(crate) fn end(&mut self) {
        if self.done {
            return;
        }
        if let Some(trailing) = self.lines.flush() {
            self.absorb_lines(vec![trailing]);
        }
        if self.done {
            return;
        }
        self.finish();
    }

    fn absorb_lines(&mut self, lines: Vec<String>) {
        for line in lines {
            let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            let event: StreamEventWire = match serde_json::from_str(payload) {
                Ok(event) => event,
                Err(source) => {
                    self.fail(ProviderFailure::Decode {
                        provider: self.provider.clone(),
                        source,
                    });
                    return;
                }
            };
            let is_message_stop = matches!(event, StreamEventWire::MessageStop {});
            match self.translator.absorb(event) {
                Ok(events) => self.pending.extend(events.into_iter().map(Ok)),
                Err(error) => {
                    self.fail(mid_stream_error_failure(&self.provider, &error));
                    return;
                }
            }
            if is_message_stop {
                self.finish();
                return;
            }
        }
    }

    fn finish(&mut self) {
        let translator = std::mem::take(&mut self.translator);
        let item = match translator.finalize() {
            Some(finished) => Ok(finished),
            None => Err(missing_finish_signal(&self.provider)),
        };
        self.pending.push_back(item);
        self.done = true;
    }

    fn fail(&mut self, failure: ProviderFailure) {
        self.pending.push_back(Err(failure));
        self.done = true;
    }

    /// The transport itself failed reading the next chunk. Queued ahead of
    /// anything [`Self::end`] would otherwise infer, since the transport
    /// failure is the more specific cause.
    pub(crate) fn fail_transport(&mut self, source: reqwest::Error) {
        let failure = transport_failure(&self.provider, source);
        self.pending.push_front(Err(failure));
        self.done = true;
    }
}

/// Maps Anthropic's documented `error.type` values to a representative HTTP
/// status, so the shared retry classification (408/429/5xx retryable) still
/// applies to an error delivered mid-stream instead of as a status code.
/// Anthropic's mid-stream errors arrive after the response already returned
/// 200, so there is no real status to read.
fn mid_stream_error_failure(provider: &str, error: &serde_json::Value) -> ProviderFailure {
    let kind = error
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let status = match kind {
        "invalid_request_error" => 400,
        "authentication_error" => 401,
        "billing_error" => 402,
        "permission_error" => 403,
        "not_found_error" => 404,
        "conflict_error" => 409,
        "request_too_large_error" => 413,
        "rate_limit_error" => 429,
        "timeout_error" => 504,
        "overloaded_error" => 529,
        _ => 500,
    };
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| error.to_string(), str::to_owned);
    ProviderFailure::Status {
        provider: provider.to_owned(),
        status,
        message,
        retry_after: None,
    }
}

fn missing_finish_signal(provider: &str) -> ProviderFailure {
    ProviderFailure::Transport {
        provider: provider.to_owned(),
        source: Box::new(std::io::Error::other(
            "completion stream ended without a finish reason",
        )),
    }
}

/// Turn an open, successful streaming `response` into the
/// [`CompletionEvent`] sequence its body describes.
pub(crate) fn event_stream(provider: String, response: reqwest::Response) -> CompletionStream {
    let state = (ChunkedReader::new(provider), Some(response));
    stream::unfold(state, poll_next).boxed()
}

type State = (ChunkedReader, Option<reqwest::Response>);

async fn poll_next(mut state: State) -> Option<(Item, State)> {
    loop {
        if let Some(item) = state.0.pop() {
            return Some((item, state));
        }
        if state.0.is_done() {
            return None;
        }
        let Some(mut response) = state.1.take() else {
            state.0.end();
            continue;
        };
        match response.chunk().await {
            Ok(Some(bytes)) => {
                state.0.absorb(&bytes);
                state.1 = Some(response);
            }
            Ok(None) => state.0.end(),
            Err(source) => state.0.fail_transport(source),
        }
    }
}

#[cfg(test)]
#[path = "tests/stream.rs"]
mod tests;
