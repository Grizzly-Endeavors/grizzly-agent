//! The contract every model backend implements.

use std::pin::Pin;

use futures_core::Stream;

use crate::completion::CompletionEvent;
use crate::error::ProviderFailure;
use crate::request::CompletionRequest;

/// A boxed, `Send` stream of completion events.
///
/// Every provider streams on the wire, so this is the one shape a completion
/// takes throughout the crate; [`crate::CompletionAccumulator`] folds it into
/// a whole [`crate::Completion`] when a caller wants that instead.
pub type CompletionStream =
    Pin<Box<dyn Stream<Item = Result<CompletionEvent, ProviderFailure>> + Send>>;

/// A model backend: turns a model id and a [`CompletionRequest`] into a
/// [`CompletionStream`].
///
/// Object-safe so a provider is chosen at runtime and held as
/// `Arc<dyn Provider>` — which is exactly what [`crate::Model`] does. A
/// provider carries no model id of its own: [`crate::Model`] passes the id
/// it was built with on every call, so one provider instance (one base URL,
/// one set of credentials) serves as many models as the endpoint offers.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Open a completion stream for `request` against `model`.
    ///
    /// # Errors
    /// Returns a [`ProviderFailure`] when the request never reaches the
    /// provider or a response never arrives — the stream never opens.
    /// Failures discovered once the stream is open arrive as `Err` items
    /// within the stream instead, so a caller already consuming events still
    /// receives every failure through one channel.
    async fn complete(
        &self,
        model: &str,
        request: CompletionRequest,
    ) -> Result<CompletionStream, ProviderFailure>;
}
