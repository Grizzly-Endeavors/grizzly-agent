//! What the OpenAI-compatible and Anthropic providers share: byte-level
//! line buffering for their server-sent-events bodies, and building the HTTP
//! client and classifying its failures.

mod http;
mod line_buffer;

pub(crate) use http::{DEFAULT_IDLE_TIMEOUT, build_client, status_failure, transport_failure};
pub(crate) use line_buffer::LineBuffer;
