//! The tool registry the turn loop advertises to the model and dispatches
//! through.

use serde_json::Value;

use crate::error::ToolFailure;
use crate::tools::context::ToolContext;
use crate::tools::handler::ToolHandler;
use crate::tools::spec::ToolSpec;

/// Two registered tools advertise the same wire name.
#[derive(Debug, thiserror::Error)]
#[error("duplicate tool name `{name}`")]
pub struct DuplicateToolName {
    /// The name two or more handlers advertised.
    pub name: String,
}

/// The registry a turn loop advertises to the model and dispatches through.
///
/// Ordered by registration and keyed by wire name: [`ToolSet::specs`]
/// advertises tools in that order, and [`ToolSet::dispatch`] looks one up by
/// the name the model used.
pub struct ToolSet {
    entries: Vec<(String, Box<dyn ToolHandler>)>,
}

impl ToolSet {
    /// Builds a registry from handlers, in the order given.
    ///
    /// # Errors
    /// Returns [`DuplicateToolName`] if two handlers advertise the same wire
    /// name.
    pub fn new(
        handlers: impl IntoIterator<Item = Box<dyn ToolHandler>>,
    ) -> Result<Self, DuplicateToolName> {
        let mut entries: Vec<(String, Box<dyn ToolHandler>)> = Vec::new();
        for handler in handlers {
            let name = handler.spec().name.into_owned();
            if entries.iter().any(|(existing, _)| existing == &name) {
                return Err(DuplicateToolName { name });
            }
            entries.push((name, handler));
        }
        Ok(Self { entries })
    }

    /// The specs to advertise to the model, in registration order.
    #[must_use]
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.entries
            .iter()
            .map(|(_, handler)| handler.spec())
            .collect()
    }

    /// The registered wire names, in registration order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Runs the named tool against raw arguments from the model.
    ///
    /// # Errors
    /// Returns [`ToolFailure::Unknown`] listing the registered names when
    /// `name` is not registered, or whatever [`ToolFailure`] the handler
    /// itself returns.
    pub async fn dispatch(
        &self,
        name: &str,
        arguments: Value,
        context: &ToolContext,
    ) -> Result<String, ToolFailure> {
        match self.entries.iter().find(|(existing, _)| existing == name) {
            Some((_, handler)) => handler.call(arguments, context).await,
            None => Err(ToolFailure::Unknown {
                name: name.to_owned(),
                available: self.names(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "tests/set.rs"]
mod tests;
