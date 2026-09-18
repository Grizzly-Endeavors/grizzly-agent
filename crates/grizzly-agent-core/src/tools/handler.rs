//! What actually runs a tool: the object-safe [`ToolHandler`] trait, and the
//! typed adapter that builds one from a [`ToolDefinition`] and a plain async
//! function.

use std::future::Future;
use std::marker::PhantomData;

use serde_json::Value;

use crate::error::ToolFailure;
use crate::tools::context::ToolContext;
use crate::tools::spec::{ToolDefinition, ToolSpec};

/// What actually runs a tool.
///
/// Object-safe so a [`crate::ToolSet`] can hold a mix of tool types behind
/// `Box<dyn ToolHandler>`. Consumer state — clients, handles, configuration —
/// is held by the handler value itself, captured when it is built, not
/// passed per call.
#[async_trait::async_trait]
pub trait ToolHandler: Send + Sync {
    /// This tool's advertisement.
    fn spec(&self) -> ToolSpec;

    /// Runs the tool against the model's raw arguments.
    ///
    /// # Errors
    /// Returns [`ToolFailure`] when the tool does not produce a useful
    /// result. This never ends the run: the failure becomes a tool result
    /// the model reads and can act on.
    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<String, ToolFailure>;
}

/// Adapts a [`ToolDefinition`] and a plain async function into a
/// [`ToolHandler`].
///
/// The function is typically a closure capturing the consumer's state. Its
/// arguments are parsed into `D::Params` before the function runs; a parse
/// failure becomes [`ToolFailure::InvalidArguments`], phrased for the model
/// to act on, and the function is never called.
pub struct TypedToolHandler<D, F> {
    handler: F,
    definition: PhantomData<fn() -> D>,
}

impl<D, F, Fut> TypedToolHandler<D, F>
where
    D: ToolDefinition,
    F: Fn(D::Params, &ToolContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolFailure>> + Send,
{
    /// Builds a handler from a tool's definition and its implementation.
    #[must_use]
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            definition: PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<D, F, Fut> ToolHandler for TypedToolHandler<D, F>
where
    D: ToolDefinition,
    F: Fn(D::Params, &ToolContext) -> Fut + Send + Sync,
    Fut: Future<Output = Result<String, ToolFailure>> + Send,
{
    fn spec(&self) -> ToolSpec {
        D::spec()
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<String, ToolFailure> {
        let params =
            serde_json::from_value(arguments).map_err(|source| ToolFailure::InvalidArguments {
                name: D::spec().name.into_owned(),
                reason: format!(
                    "arguments do not match this tool's parameter schema ({source}) — \
                     re-read the schema and resend corrected arguments"
                ),
            })?;
        (self.handler)(params, context).await
    }
}

#[cfg(test)]
#[path = "tests/handler.rs"]
mod tests;
