//! [`OpenAiCompatibleProvider`]: the [`grizzly_agent_core::Provider`] for the
//! OpenAI-compatible chat-completions API.

use std::time::Duration;

use grizzly_agent_core::{CompletionRequest, CompletionStream, Provider, ProviderFailure};

use super::stream;
use super::wire;
use crate::transport::{DEFAULT_IDLE_TIMEOUT, build_client, status_failure, transport_failure};

/// A [`Provider`] for any OpenAI-compatible chat-completions endpoint —
/// OpenAI itself, vLLM, Ollama, or another gateway that speaks the same wire
/// shape.
///
/// Carries no model id of its own: the base URL, key, and idle timeout are
/// its only configuration, so one instance serves every model the endpoint
/// offers — [`grizzly_agent_core::Model`] supplies the model id on every
/// call.
pub struct OpenAiCompatibleProvider {
    http: reqwest::Client,
    completions_url: String,
    api_key: Option<String>,
}

impl std::fmt::Debug for OpenAiCompatibleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleProvider")
            .field("completions_url", &self.completions_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

impl OpenAiCompatibleProvider {
    /// Start building a provider at `base_url` — a base such as
    /// `http://localhost:11434/v1`; `/chat/completions` is appended.
    #[must_use]
    pub fn builder(base_url: impl Into<String>) -> OpenAiCompatibleProviderBuilder {
        OpenAiCompatibleProviderBuilder {
            base_url: base_url.into(),
            api_key: None,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        }
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn complete(
        &self,
        model: &str,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure> {
        let body = wire::build_request(&request, model);
        let mut call = self.http.post(&self.completions_url);
        if let Some(key) = &self.api_key {
            call = call.bearer_auth(key);
        }
        let response = call
            .json(&body)
            .send()
            .await
            .map_err(|source| transport_failure(&self.completions_url, source))?;

        if !response.status().is_success() {
            return Err(status_failure(&self.completions_url, response).await);
        }
        Ok(stream::event_stream(self.completions_url.clone(), response))
    }
}

/// Builds an [`OpenAiCompatibleProvider`]. Obtained from
/// [`OpenAiCompatibleProvider::builder`].
pub struct OpenAiCompatibleProviderBuilder {
    base_url: String,
    api_key: Option<String>,
    idle_timeout: Duration,
}

impl OpenAiCompatibleProviderBuilder {
    /// Send this key as a bearer token on every request. Omit it for an
    /// endpoint that needs none.
    #[must_use]
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// How long a stream may sit idle between reads before it fails
    /// retryably. Defaults to 45 seconds.
    #[must_use]
    pub fn idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    /// Finish building the provider.
    ///
    /// # Errors
    /// Returns [`ProviderFailure::Configuration`] if `base_url` is not a
    /// usable URL, or if the HTTP client cannot be built.
    pub fn build(self) -> Result<OpenAiCompatibleProvider, ProviderFailure> {
        let joined = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let completions_url = reqwest::Url::parse(&joined)
            .map_err(|source| {
                ProviderFailure::Configuration(format!("invalid base url {joined}: {source}"))
            })?
            .to_string();
        let http = build_client(self.idle_timeout, reqwest::header::HeaderMap::new())?;
        Ok(OpenAiCompatibleProvider {
            http,
            completions_url,
            api_key: self.api_key,
        })
    }
}

#[cfg(test)]
#[path = "tests/provider.rs"]
mod tests;
