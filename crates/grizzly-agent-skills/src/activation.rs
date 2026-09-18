//! Shared skill activation state, the two [`grizzly_agent_core::ToolHandler`]s
//! a consumer adds to their `ToolSet`, and the [`DynamicSection`] that
//! renders the index and every active skill's body.
//!
//! A consumer builds a [`SkillIndex`], wraps it in [`SkillState::new`],
//! shares one [`Arc`] of that state across [`activate_skill_tool`],
//! [`deactivate_skill_tool`], and a [`SkillsSection`] added to their
//! `Agent`, and gets progressive disclosure with no further code.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Deserialize;

use grizzly_agent_core::{
    DynamicSection, ToolContext, ToolDefinition, ToolFailure, ToolHandler, ToolSpec,
    TypedToolHandler,
};

use crate::index::SkillIndex;

/// Shared state behind skill activation: the scanned index (read-only after
/// construction) and the set of currently active skills, each holding the
/// body text that was loaded from disk when it was activated.
///
/// Wrap in an [`Arc`] and clone that into [`activate_skill_tool`],
/// [`deactivate_skill_tool`], and [`SkillsSection`] so all three share one
/// active set.
pub struct SkillState {
    index: SkillIndex,
    active: Mutex<BTreeMap<String, String>>,
}

impl SkillState {
    /// Builds shared activation state around a scanned index, with no
    /// skills active yet.
    #[must_use]
    pub fn new(index: SkillIndex) -> Self {
        Self {
            index,
            active: Mutex::new(BTreeMap::new()),
        }
    }

    /// The scanned index this state activates skills from.
    #[must_use]
    pub fn index(&self) -> &SkillIndex {
        &self.index
    }

    /// Loads `name`'s body from disk and adds it to the active set.
    async fn activate(&self, name: &str) -> Result<(), ActivationError> {
        let Some(directory) = self.index.directory(name) else {
            return Err(ActivationError::UnknownSkill {
                name: name.to_owned(),
                available: self.index.names(),
            });
        };
        let path = directory.join("SKILL.md");
        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|source| ActivationError::Io {
                    name: name.to_owned(),
                    path: path.clone(),
                    source,
                })?;
        let (_, body) = grizzly_agent_prompts::split_frontmatter(&content).ok_or_else(|| {
            ActivationError::MissingDelimiters {
                name: name.to_owned(),
                path: path.clone(),
            }
        })?;
        lock(&self.active).insert(name.to_owned(), body.trim().to_owned());
        Ok(())
    }

    /// Removes `name` from the active set. Returns whether it had been
    /// active — deactivating a skill that was not active is a harmless
    /// no-op, not a failure.
    fn deactivate(&self, name: &str) -> bool {
        lock(&self.active).remove(name).is_some()
    }

    /// The index's compact listing, followed by every active skill's body.
    fn render(&self) -> String {
        let index_listing = self.index.render();
        let active = lock(&self.active);
        if active.is_empty() {
            return index_listing;
        }
        let bodies = active
            .iter()
            .map(|(name, body)| format!("## {name}\n\n{body}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        if index_listing.is_empty() {
            bodies
        } else {
            format!("{index_listing}\n{bodies}")
        }
    }
}

fn lock(active: &Mutex<BTreeMap<String, String>>) -> MutexGuard<'_, BTreeMap<String, String>> {
    active.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Why loading a skill's body for activation failed.
#[derive(Debug, thiserror::Error)]
enum ActivationError {
    #[error("no skill named `{name}`")]
    UnknownSkill {
        name: String,
        available: Vec<String>,
    },
    #[error("could not read `{}` for skill `{name}`: {source}", path.display())]
    Io {
        name: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "`{}` for skill `{name}` is missing its frontmatter delimiters",
        path.display()
    )]
    MissingDelimiters { name: String, path: PathBuf },
}

impl ActivationError {
    /// Renders this failure as what the model reads back: a correctable
    /// argument problem for an unknown skill name, an execution problem for
    /// everything else.
    fn into_tool_failure(self, tool_name: &'static str) -> ToolFailure {
        let message = self.to_string();
        match self {
            Self::UnknownSkill { name, available } => ToolFailure::InvalidArguments {
                name: tool_name.to_owned(),
                reason: format!(
                    "no skill named `{name}`; available skills: {}",
                    available.join(", ")
                ),
            },
            Self::Io { .. } | Self::MissingDelimiters { .. } => ToolFailure::Execution {
                name: tool_name.to_owned(),
                message,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillNameParams {
    name: String,
}

struct ActivateSkillDefinition;

impl ToolDefinition for ActivateSkillDefinition {
    type Params = SkillNameParams;

    fn spec() -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("activate_skill"),
            description: Cow::Borrowed(
                "Activates a skill by name, loading its full instructions into the system \
                 prompt for the rest of this run. Call this once the skills listing shows a \
                 skill relevant to the current task.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "the skill's name, exactly as shown in the skills listing"
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        }
    }
}

struct DeactivateSkillDefinition;

impl ToolDefinition for DeactivateSkillDefinition {
    type Params = SkillNameParams;

    fn spec() -> ToolSpec {
        ToolSpec {
            name: Cow::Borrowed("deactivate_skill"),
            description: Cow::Borrowed(
                "Deactivates a previously activated skill, removing its instructions from the \
                 system prompt.",
            ),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "the active skill's name to deactivate"
                    }
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        }
    }
}

/// A [`ToolHandler`] that activates a skill by name, loading its body from
/// disk into `state`'s active set.
///
/// Fails with [`ToolFailure::InvalidArguments`] naming the available skills
/// when the model asks for a name the index does not contain, or
/// [`ToolFailure::Execution`] if the skill's `SKILL.md` cannot be read at
/// activation time.
#[must_use]
pub fn activate_skill_tool(state: Arc<SkillState>) -> Box<dyn ToolHandler> {
    Box::new(TypedToolHandler::<ActivateSkillDefinition, _>::new(
        move |params: SkillNameParams, _context: &ToolContext| {
            let state = Arc::clone(&state);
            async move {
                state
                    .activate(&params.name)
                    .await
                    .map_err(|error| error.into_tool_failure("activate_skill"))?;
                Ok(format!("activated skill `{}`", params.name))
            }
        },
    ))
}

/// A [`ToolHandler`] that deactivates a skill by name, removing its body
/// from `state`'s active set. Deactivating a skill that was not active
/// succeeds with a message saying so, rather than failing.
#[must_use]
pub fn deactivate_skill_tool(state: Arc<SkillState>) -> Box<dyn ToolHandler> {
    Box::new(TypedToolHandler::<DeactivateSkillDefinition, _>::new(
        move |params: SkillNameParams, _context: &ToolContext| {
            let state = Arc::clone(&state);
            async move {
                let was_active = state.deactivate(&params.name);
                Ok(if was_active {
                    format!("deactivated skill `{}`", params.name)
                } else {
                    format!("skill `{}` was not active", params.name)
                })
            }
        },
    ))
}

/// A [`DynamicSection`] rendering the skill index's compact listing plus
/// every currently active skill's full body, rendered fresh before every
/// model call — so a skill activated earlier in a run is visible on the
/// next round.
pub struct SkillsSection {
    state: Arc<SkillState>,
}

impl SkillsSection {
    /// Wraps `state` in a section an [`grizzly_agent_core::Agent`] renders
    /// into its system prompt.
    #[must_use]
    pub fn new(state: Arc<SkillState>) -> Self {
        Self { state }
    }
}

impl DynamicSection for SkillsSection {
    fn render(&self) -> String {
        self.state.render()
    }
}

#[cfg(test)]
#[path = "tests/activation.rs"]
mod tests;
