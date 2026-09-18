//! The validated in-memory model of a prompt tree.
//!
//! This is the output of [`crate::load`] and the input later phases turn into
//! generated code. Everything here has already passed validation: cross-file
//! references are resolved, wire names are computed, and bodies are tokenized
//! into literal/placeholder segments ready to interleave with field values.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Every prompt file in a directory, keyed by id in sorted order for
/// deterministic downstream codegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTree {
    /// Every validated file, keyed by its id.
    pub files: BTreeMap<String, PromptFile>,
}

/// One validated prompt file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptFile {
    /// The file's declared id — also the generated Rust type name.
    pub id: String,
    /// Path relative to the prompt-directory root, for diagnostics.
    pub path: PathBuf,
    /// This file's type-specific content.
    pub kind: PromptKind,
    /// The human-facing annotations block.
    pub annotations: Annotations,
}

/// The three file types, each carrying only what its type permits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    /// Model-visible prose with typed placeholders.
    Prompt {
        /// The body, tokenized into literal and placeholder segments.
        body: Vec<BodySegment>,
        /// Declared variables in body first-appearance order.
        variables: Vec<Variable>,
    },
    /// A tool definition: a static description body plus a parameter schema.
    Tool {
        /// The wire name sent to the model (explicit `name`, else `snake_case(id)`).
        wire_name: String,
        /// Whether `wire_name` came from an explicit `name` field.
        name_explicit: bool,
        /// The tool description sent to the model. Static (no placeholders).
        description: String,
        /// Where this tool's parameter schema comes from.
        schema: ToolSchemaRef,
    },
    /// A shared parameter shape referenced by one or more tools. Its body is
    /// unused — a params file exists only to define a struct.
    Params {
        /// The shared parameter schema.
        schema: ToolSchema,
    },
}

/// Where a tool's parameter schema comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSchemaRef {
    /// Defined inline on the tool.
    Inline(ToolSchema),
    /// The id of a `type: params` file whose struct this tool shares.
    Shared(String),
}

/// An ordered parameter list. Order follows the YAML, which downstream codegen
/// preserves so struct field order stays stable across builds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolSchema {
    /// The parameters, in declaration order.
    pub params: Vec<(String, Param)>,
}

/// One parameter definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// The parameter's type.
    pub ty: ParamType,
    /// The parameter's description, shown to the model.
    pub description: String,
    /// Whether the model may omit this parameter.
    pub optional: bool,
}

/// The closed parameter type vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    /// A JSON string.
    String,
    /// A JSON integer.
    Integer,
    /// A JSON number.
    Number,
    /// A JSON boolean.
    Boolean,
    /// A closed set of `snake_case` string values, surfaced as a generated enum.
    Enum {
        /// The permitted wire values, in declaration order.
        values: Vec<String>,
    },
}

/// A run of body text: either a literal segment or a `{{placeholder}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodySegment {
    /// Literal text, sent verbatim.
    Literal(String),
    /// A `{{name}}` placeholder, filled in at render time.
    Placeholder(String),
}

/// One `annotations.variables` entry, enriched with its placeholder name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// The placeholder name this entry documents.
    pub name: String,
    /// Where the value comes from.
    pub source: String,
    /// What the value contains, including any fallback for absent data.
    pub contents: String,
}

/// The human-facing annotations block. Never sent to the model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Annotations {
    /// When this prompt is sent, for prompts and tools.
    pub sent_when: Option<String>,
    /// The call sites that reference this prompt.
    pub used_by: Vec<UsedBy>,
    /// Notes on why the prompt is written the way it is.
    pub reasoning: Vec<String>,
}

/// One call site: a source file and the function within it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsedBy {
    /// The source file, relative to the consumer's `src/` directory.
    pub file: String,
    /// The function within that file.
    pub function: String,
}
