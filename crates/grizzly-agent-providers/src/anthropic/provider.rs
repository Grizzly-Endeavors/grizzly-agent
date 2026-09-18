//! [`AnthropicProvider`]: the [`grizzly_agent_core::Provider`] for
//! Anthropic's Messages API.

use std::time::Duration;

use grizzly_agent_core::{CompletionRequest, CompletionStream, Provider, ProviderFailure};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::stream;
use super::wire;
use crate::transport::{DEFAULT_IDLE_TIMEOUT, build_client, status_failure, transport_failure};

/// The Anthropic API version this provider speaks on the wire, per
/// `platform.claude.com/docs/en/api/overview` — current and stable since
/// the Messages API's original release; verified current as of this
/// provider's implementation.
const DEFAULT_API_VERSION: &str = "2023-06-01";

/// Anthropic requires `max_tokens` on every request; the OpenAI-compatible
/// wire treats it as optional. This is the fallback used when neither the
/// request nor `Model`'s own default sets one.
const DEFAULT_MAX_TOKENS: u32 = 4096;

const MESSAGES_PATH: &str = "/v1/messages";

/// A [`Provider`] for Anthropic's Messages API.
///
/// Carries no model id of its own, the same way as
/// [`crate::OpenAiCompatibleProvider`]: [`grizzly_agent_core::Model`]
/// supplies the model id on every call, so one provider instance serves as
/// many models as the account can call.
pub struct AnthropicProvider {
    http: reqwest::Client,
    messages_url: String,
    default_max_tokens: u32,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("messages_url", &self.messages_url)
            .finish_non_exhaustive()
    }
}

impl AnthropicProvider {
    /// Start building a provider authenticating with `api_key`.
    #[must_use]
    pub fn builder(api_key: impl Into<String>) -> AnthropicProviderBuilder {
        AnthropicProviderBuilder {
            base_url: "https://api.anthropic.com".to_owned(),
            api_key: api_key.into(),
            api_version: DEFAULT_API_VERSION.to_owned(),
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            default_max_tokens: DEFAULT_MAX_TOKENS,
        }
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicProvider {
    async fn complete(
        &self,
        model: &str,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure> {
        let body = wire::build_request(&request, model, self.default_max_tokens);
        let response = self
            .http
            .post(&self.messages_url)
            .json(&body)
            .send()
            .await
            .map_err(|source| transport_failure(&self.messages_url, source))?;

        if !response.status().is_success() {
            return Err(status_failure(&self.messages_url, response).await);
        }
        Ok(stream::event_stream(self.messages_url.clone(), response))
    }
}

/// Builds an [`AnthropicProvider`]. Obtained from
/// [`AnthropicProvider::builder`].
pub struct AnthropicProviderBuilder {
    base_url: String,
    api_key: String,
    api_version: String,
    idle_timeout: Duration,
    default_max_tokens: u32,
}

impl AnthropicProviderBuilder {
    /// Override the API base URL — for a proxy or a compatible gateway.
    /// Defaults to `https://api.anthropic.com`.
    #[must_use]
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Override the `anthropic-version` header. Defaults to the current
    /// stable version.
    #[must_use]
    pub fn api_version(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = api_version.into();
        self
    }

    /// How long a stream may sit idle between reads before it fails
    /// retryably. Defaults to 45 seconds.
    #[must_use]
    pub fn idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    /// `max_tokens` to send when a request sets none. Anthropic requires the
    /// field on every call; this is the value used when neither the request
    /// nor `Model`'s own default supplies one. Defaults to 4096.
    #[must_use]
    pub fn default_max_tokens(mut self, default_max_tokens: u32) -> Self {
        self.default_max_tokens = default_max_tokens;
        self
    }

    /// Finish building the provider.
    ///
    /// # Errors
    /// Returns [`ProviderFailure::Configuration`] if `base_url` is not a
    /// usable URL, the API key or version cannot be sent as a header value,
    /// or the HTTP client cannot be built.
    pub fn build(self) -> Result<AnthropicProvider, ProviderFailure> {
        let joined = format!("{}{MESSAGES_PATH}", self.base_url.trim_end_matches('/'));
        let messages_url = reqwest::Url::parse(&joined)
            .map_err(|source| {
                ProviderFailure::Configuration(format!("invalid base url {joined}: {source}"))
            })?
            .to_string();
        let headers = default_headers(&self.api_key, &self.api_version)?;
        let http = build_client(self.idle_timeout, headers)?;
        Ok(AnthropicProvider {
            http,
            messages_url,
            default_max_tokens: self.default_max_tokens,
        })
    }
}

fn default_headers(api_key: &str, api_version: &str) -> Result<HeaderMap, ProviderFailure> {
    let mut headers = HeaderMap::new();
    let key_value = HeaderValue::from_str(api_key).map_err(|source| {
        ProviderFailure::Configuration(format!("api key is not a valid header value: {source}"))
    })?;
    let version_value = HeaderValue::from_str(api_version).map_err(|source| {
        ProviderFailure::Configuration(format!(
            "anthropic-version is not a valid header value: {source}"
        ))
    })?;
    headers.insert(HeaderName::from_static("x-api-key"), key_value);
    headers.insert(HeaderName::from_static("anthropic-version"), version_value);
    Ok(headers)
}

#[cfg(test)]
#[path = "tests/provider.rs"]
mod tests;
