//! Building the shared HTTP client and turning its failures into
//! [`ProviderFailure`].

use std::time::Duration;

use grizzly_agent_core::ProviderFailure;
use reqwest::Response;
use reqwest::header::{HeaderMap, RETRY_AFTER};

/// How long a stream may sit idle between reads before it is treated as
/// failed, unless a provider's constructor sets its own. Reset on every read
/// that returns data — a model that thinks for minutes is fine as long as it
/// keeps the connection alive; a connection an intermediary silently dropped
/// is caught here instead of hanging until the process gives up.
pub(crate) const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(45);

/// Build the `reqwest` client both providers send through: rustls, JSON
/// bodies, `idle_timeout` bound to the gap between reads on the response
/// body (not the whole call — a long generation that keeps streaming stays
/// alive past it), and `default_headers` sent on every request (auth and
/// version headers that never change per call).
///
/// # Errors
/// Returns [`ProviderFailure::Configuration`] if the client cannot be built
/// (this reqwest build has no TLS backend, or the platform has none usable).
pub(crate) fn build_client(
    idle_timeout: Duration,
    default_headers: HeaderMap,
) -> Result<reqwest::Client, ProviderFailure> {
    reqwest::Client::builder()
        .read_timeout(idle_timeout)
        .default_headers(default_headers)
        .build()
        .map_err(|source| {
            ProviderFailure::Configuration(format!("failed to build http client: {source}"))
        })
}

/// The wait a `Retry-After` response header asks for, if present and a plain
/// integer count of seconds — the form every provider this crate speaks
/// sends. The HTTP-date form is not parsed: none of them use it.
pub(crate) fn retry_after_from_headers(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    let seconds: u64 = value.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

/// Turn a non-success HTTP response into a [`ProviderFailure::Status`],
/// reading its `Retry-After` header and body.
pub(crate) async fn status_failure(provider: &str, response: Response) -> ProviderFailure {
    let status = response.status();
    let retry_after = retry_after_from_headers(response.headers());
    let message = match response.text().await {
        Ok(body) => body,
        Err(err) => format!("<body unreadable: {err}>"),
    };
    ProviderFailure::Status {
        provider: provider.to_owned(),
        status: status.as_u16(),
        message,
        retry_after,
    }
}

/// Wrap a transport-level `reqwest` failure — connect, send, or an idle read
/// timing out — as a retryable [`ProviderFailure::Transport`].
pub(crate) fn transport_failure(provider: &str, source: reqwest::Error) -> ProviderFailure {
    ProviderFailure::Transport {
        provider: provider.to_owned(),
        source: Box::new(source),
    }
}

#[cfg(test)]
#[path = "tests/http.rs"]
mod tests;
