//! Drives a streaming HTTP response through [`super::wire`]'s translation
//! into a [`CompletionStream`].
//!
//! Split in two so the reassembly logic is testable without a socket:
//! [`ChunkedReader`] is the pure, synchronous reducer — feed it transport
//! bytes as they arrive and it queues [`CompletionEvent`]s and failures in
//! order — and [`event_stream`] is the thin async wrapper that reads a
//! `reqwest::Response` and drives the reader with what it returns.

use std::collections::VecDeque;

use futures_util::stream::{self, StreamExt};
use grizzly_agent_core::{CompletionEvent, CompletionStream, ProviderFailure};

use super::wire::{ChunkWire, EventTranslator};
use crate::transport::{LineBuffer, transport_failure};

type Item = Result<CompletionEvent, ProviderFailure>;

/// Turns transport bytes into a queue of [`CompletionEvent`]s and failures,
/// with no I/O of its own.
pub(crate) struct ChunkedReader {
    provider: String,
    lines: LineBuffer,
    translator: EventTranslator,
    pending: VecDeque<Item>,
    /// Set once nothing further will ever be queued: a `[DONE]` line, a
    /// clean close, or a failure all end the reader the same way.
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

    /// Whether every event this reader will ever produce has already been
    /// queued (via [`Self::pop`]) or is queued now.
    pub(crate) fn is_done(&self) -> bool {
        self.done && self.pending.is_empty()
    }

    /// Take the next queued item, if any.
    pub(crate) fn pop(&mut self) -> Option<Item> {
        self.pending.pop_front()
    }

    /// Feed one transport read in. A malformed chunk or a mid-stream error
    /// payload queues that failure as the reader's final item; a `[DONE]`
    /// line ends the reader the same way [`Self::end`] does.
    pub(crate) fn absorb(&mut self, bytes: &[u8]) {
        if self.done {
            return;
        }
        let lines = self.lines.push(bytes);
        self.absorb_lines(lines);
    }

    /// The transport closed cleanly (or the caller otherwise knows no more
    /// bytes are coming): flush any unterminated trailing line, then release
    /// the buffered finish event, or fail retryably when the stream ended
    /// without ever seeing a finish reason.
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
        let translator = std::mem::take(&mut self.translator);
        let item = match translator.finalize() {
            Some(finished) => Ok(finished),
            None => Err(missing_finish_signal(&self.provider)),
        };
        self.pending.push_back(item);
        self.done = true;
    }

    fn absorb_lines(&mut self, lines: Vec<String>) {
        for line in lines {
            let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
                continue;
            };
            if payload.is_empty() {
                continue;
            }
            if payload == "[DONE]" {
                self.end();
                return;
            }
            let chunk: ChunkWire = match serde_json::from_str(payload) {
                Ok(chunk) => chunk,
                Err(source) => {
                    self.fail(ProviderFailure::Decode {
                        provider: self.provider.clone(),
                        source,
                    });
                    return;
                }
            };
            if let Some(error) = chunk.error {
                self.fail(mid_stream_error_failure(&self.provider, &error));
                return;
            }
            self.pending
                .extend(self.translator.absorb(chunk).into_iter().map(Ok));
        }
    }

    fn fail(&mut self, failure: ProviderFailure) {
        self.pending.push_back(Err(failure));
        self.done = true;
    }

    /// The transport itself failed reading the next chunk — a connect
    /// error, a dropped connection, or an idle read past the configured
    /// timeout. Queued ahead of anything [`Self::end`] would otherwise infer
    /// (a missing finish reason), since the transport failure is the more
    /// specific cause.
    pub(crate) fn fail_transport(&mut self, source: reqwest::Error) {
        let failure = transport_failure(&self.provider, source);
        self.pending.push_front(Err(failure));
        self.done = true;
    }
}

/// An error payload delivered mid-stream instead of as an HTTP status.
/// Treated as the endpoint rejecting the request outright rather than a
/// transient failure: retrying an identical request that was already
/// rejected mid-generation is more likely to repeat the rejection than
/// resolve it.
fn mid_stream_error_failure(provider: &str, error: &serde_json::Value) -> ProviderFailure {
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| error.to_string(), str::to_owned);
    ProviderFailure::Status {
        provider: provider.to_owned(),
        status: 400,
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
