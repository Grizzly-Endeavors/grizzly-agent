//! [`Model`]: a provider, a model identifier, default parameters and a retry
//! policy, bundled into the one thing the rest of the crate and every
//! consumer calls a model through.

use std::future::ready;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{StreamExt, stream};
use tokio::time::Instant;

use crate::accumulator::CompletionAccumulator;
use crate::completion::{Completion, CompletionEvent};
use crate::error::ProviderFailure;
use crate::provider::{CompletionStream, Provider};
use crate::request::{CompletionRequest, ResponseFormat};
use crate::retry::{self, JitterFn, RetryPolicy, SleepFn, run_with_retry};

/// A shared provider, a model identifier, default request parameters and a
/// retry policy — what the rest of the crate and every consumer passes
/// around to call a model. Cheap to clone.
///
/// "Use this model from this provider" is constructing a `Model`; nothing
/// downstream of that knows which provider it is.
#[derive(Clone)]
pub struct Model {
    provider: Arc<dyn Provider>,
    identifier: String,
    default_max_tokens: Option<u32>,
    default_temperature: Option<f32>,
    default_response_format: Option<ResponseFormat>,
    retry_policy: RetryPolicy,
    timeout: Option<Duration>,
    sleep: SleepFn,
    jitter: JitterFn,
}

impl Model {
    /// Start building a `Model` for `provider`, reporting itself as `model`
    /// to callers.
    #[must_use]
    pub fn builder(provider: Arc<dyn Provider>, model: impl Into<String>) -> ModelBuilder {
        ModelBuilder {
            provider,
            identifier: model.into(),
            default_max_tokens: None,
            default_temperature: None,
            default_response_format: None,
            retry_policy: RetryPolicy::default(),
            timeout: None,
            sleep: retry::tokio_sleep(),
            jitter: retry::full_jitter(),
        }
    }

    /// The model identifier this `Model` was built with.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.identifier
    }

    /// Complete `request`: open the stream and drive it through
    /// [`CompletionAccumulator`], buffering the whole reply before
    /// returning. A failure at any point — before the stream opens or
    /// mid-stream — retries the whole request per this model's retry policy.
    ///
    /// # Errors
    /// [`ProviderFailure::InvalidRequest`] if `request` breaks a request
    /// invariant; otherwise the provider's failure once retries (if any) are
    /// exhausted, or once the configured timeout elapses.
    pub async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<Completion, ProviderFailure> {
        request.validate()?;
        let request = self.apply_defaults(request);
        with_timeout(
            self.timeout,
            &self.identifier,
            self.complete_with_retry(&request),
        )
        .await
    }

    async fn complete_with_retry(
        &self,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderFailure> {
        run_with_retry(&self.retry_policy, &self.sleep, &self.jitter, |_attempt| {
            self.complete_once(request)
        })
        .await
    }

    async fn complete_once(
        &self,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderFailure> {
        let mut events = self.provider.complete(request.clone()).await?;
        let mut accumulator = CompletionAccumulator::new();
        while let Some(event) = events.next().await {
            accumulator.push(event?);
        }
        accumulator.finish()
    }

    /// Open a completion stream for `request`. Retries only failures that
    /// happen before the first event is yielded to the caller: once any
    /// event has been delivered, a later failure is the stream's final item
    /// and is not retried, because the caller has already consumed partial
    /// output a retry could not take back.
    ///
    /// # Errors
    /// [`ProviderFailure::InvalidRequest`] if `request` breaks a request
    /// invariant; otherwise the provider's failure if every attempt at
    /// producing a first event fails, or once the configured timeout elapses
    /// before one arrives.
    pub async fn stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure> {
        request.validate()?;
        let request = self.apply_defaults(request);
        let deadline = self.timeout.map(|budget| Deadline {
            at: Instant::now() + budget,
        });

        let (first_event, rest) = self.open_with_deadline(&request, deadline).await?;
        let tail = match deadline {
            Some(deadline) => bound_by_deadline(rest, deadline, self.identifier.clone()),
            None => rest,
        };
        Ok(stream::once(ready(Ok(first_event))).chain(tail).boxed())
    }

    async fn open_with_deadline(
        &self,
        request: &CompletionRequest,
        deadline: Option<Deadline>,
    ) -> Result<(CompletionEvent, CompletionStream), ProviderFailure> {
        let opening = self.open_with_retry(request);
        match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline.at, opening).await {
                Ok(result) => result,
                Err(_elapsed) => Err(timeout_failure(&self.identifier)),
            },
            None => opening.await,
        }
    }

    async fn open_with_retry(
        &self,
        request: &CompletionRequest,
    ) -> Result<(CompletionEvent, CompletionStream), ProviderFailure> {
        run_with_retry(&self.retry_policy, &self.sleep, &self.jitter, |_attempt| {
            self.open_once(request)
        })
        .await
    }

    async fn open_once(
        &self,
        request: &CompletionRequest,
    ) -> Result<(CompletionEvent, CompletionStream), ProviderFailure> {
        let mut events = self.provider.complete(request.clone()).await?;
        match events.next().await {
            Some(Ok(event)) => Ok((event, events)),
            Some(Err(failure)) => Err(failure),
            None => Err(transport_failure(
                &self.identifier,
                "completion stream ended with no events",
            )),
        }
    }

    fn apply_defaults(&self, mut request: CompletionRequest) -> CompletionRequest {
        request.max_tokens = request.max_tokens.or(self.default_max_tokens);
        request.temperature = request.temperature.or(self.default_temperature);
        if request.response_format.is_none() {
            request
                .response_format
                .clone_from(&self.default_response_format);
        }
        request
    }
}

/// Builds a [`Model`]. Obtained from [`Model::builder`].
pub struct ModelBuilder {
    provider: Arc<dyn Provider>,
    identifier: String,
    default_max_tokens: Option<u32>,
    default_temperature: Option<f32>,
    default_response_format: Option<ResponseFormat>,
    retry_policy: RetryPolicy,
    timeout: Option<Duration>,
    sleep: SleepFn,
    jitter: JitterFn,
}

impl ModelBuilder {
    /// The output-token limit a request falls back to when it sets none.
    #[must_use]
    pub fn default_max_tokens(mut self, max_tokens: u32) -> Self {
        self.default_max_tokens = Some(max_tokens);
        self
    }

    /// The sampling temperature a request falls back to when it sets none.
    #[must_use]
    pub fn default_temperature(mut self, temperature: f32) -> Self {
        self.default_temperature = Some(temperature);
        self
    }

    /// The response format a request falls back to when it sets none.
    #[must_use]
    pub fn default_response_format(mut self, response_format: ResponseFormat) -> Self {
        self.default_response_format = Some(response_format);
        self
    }

    /// The retry policy this model applies to `complete` and `stream`.
    /// Defaults to [`RetryPolicy::default`]; pass [`RetryPolicy::none`] to
    /// disable retries.
    #[must_use]
    pub fn retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// A per-call timeout covering the whole call: for `stream`, from the
    /// call until the returned stream ends.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Finish building the `Model`.
    #[must_use]
    pub fn build(self) -> Model {
        Model {
            provider: self.provider,
            identifier: self.identifier,
            default_max_tokens: self.default_max_tokens,
            default_temperature: self.default_temperature,
            default_response_format: self.default_response_format,
            retry_policy: self.retry_policy,
            timeout: self.timeout,
            sleep: self.sleep,
            jitter: self.jitter,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_sleep(mut self, sleep: SleepFn) -> Self {
        self.sleep = sleep;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_jitter(mut self, jitter: JitterFn) -> Self {
        self.jitter = jitter;
        self
    }
}

#[derive(Debug, Clone, Copy)]
struct Deadline {
    at: Instant,
}

fn bound_by_deadline(
    inner: CompletionStream,
    deadline: Deadline,
    model: String,
) -> CompletionStream {
    stream::unfold(Some((inner, model)), move |remaining| async move {
        let (mut source, label) = remaining?;
        match tokio::time::timeout_at(deadline.at, source.next()).await {
            Ok(Some(item)) => Some((item, Some((source, label)))),
            Ok(None) => None,
            Err(_elapsed) => Some((Err(timeout_failure(&label)), None)),
        }
    })
    .boxed()
}

async fn with_timeout<T, F>(
    timeout: Option<Duration>,
    model: &str,
    future: F,
) -> Result<T, ProviderFailure>
where
    F: std::future::Future<Output = Result<T, ProviderFailure>>,
{
    match timeout {
        Some(duration) => match tokio::time::timeout(duration, future).await {
            Ok(result) => result,
            Err(_elapsed) => Err(timeout_failure(model)),
        },
        None => future.await,
    }
}

fn timeout_failure(model: &str) -> ProviderFailure {
    transport_failure(model, "model call timed out")
}

fn transport_failure(model: &str, detail: &str) -> ProviderFailure {
    ProviderFailure::Transport {
        provider: model.to_owned(),
        source: Box::new(std::io::Error::other(detail)),
    }
}

#[cfg(test)]
#[path = "tests/model.rs"]
mod tests;
