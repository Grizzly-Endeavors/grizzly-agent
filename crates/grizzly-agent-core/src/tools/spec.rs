//! The advertisement sent to the model, and the trait binding a tool type to
//! its schema and its parameter type.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::borrow::Cow;

/// The advertisement sent to the model: a wire name, a description, and a
/// JSON Schema for its parameters.
///
/// Name and description are [`Cow<'static, str>`] so generated tools use
/// borrowed statics at zero cost, while tools built at runtime — skills,
/// MCP-backed tools a consumer wraps, anything discovered from config — own
/// their strings.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// The name the model calls this tool by.
    pub name: Cow<'static, str>,
    /// What the tool does, shown to the model.
    pub description: Cow<'static, str>,
    /// The JSON Schema its arguments must satisfy.
    pub parameters: serde_json::Value,
}

/// Binds a tool type to its [`ToolSpec`] and the type its arguments parse
/// into.
///
/// Codegen emits this for every generated tool, so the schema advertised to
/// the model and the type its arguments parse into come from the same
/// prompt file and cannot drift apart.
pub trait ToolDefinition {
    /// The type the model's arguments deserialize into.
    type Params: DeserializeOwned;

    /// This tool's advertisement.
    #[must_use]
    fn spec() -> ToolSpec;
}

/// The parameter type for a tool that takes no arguments.
///
/// Rejects unknown fields, so a model that sends unexpected arguments to a
/// no-parameter tool gets a schema-mismatch [`crate::ToolFailure`] rather
/// than having them silently ignored.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "the brackets are load-bearing: serde deserializes a bracketed struct from a \
              JSON object (the `{}` a model actually sends for a no-argument tool call), \
              where a unit struct would only accept a JSON `null`"
)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[cfg(test)]
#[path = "tests/spec.rs"]
mod tests;
