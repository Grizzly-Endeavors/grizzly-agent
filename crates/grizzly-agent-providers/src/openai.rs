//! The OpenAI-compatible chat-completions provider: a base URL and an
//! optional API key serve OpenAI itself, vLLM, Ollama, or any other
//! OpenAI-compatible gateway.

mod provider;
mod stream;
mod wire;

pub use provider::{OpenAiCompatibleProvider, OpenAiCompatibleProviderBuilder};
