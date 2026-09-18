//! Parses a `SKILL.md` file's frontmatter into the six fields the open
//! [Agent Skills specification](https://agentskills.io/specification)
//! defines, validated to its constraints.
//!
//! This module does no file I/O: [`parse_skill`] takes the file's content
//! and its directory name (needed only to check `name` against it) and
//! returns a parsed, validated [`Skill`] or a typed [`SkillError`]. The
//! [`crate::index`] module supplies both from disk.

use std::collections::BTreeMap;

use yaml_rust2::{Yaml, yaml::Hash};

/// The frontmatter fields the open Agent Skills specification defines.
///
/// Only `name` and `description` are required. `metadata` and
/// `allowed_tools` default to empty when absent. `extra` holds every
/// frontmatter key outside the six the spec defines — preserved, not
/// rejected, so a skill written for another product (Claude Code's
/// `argument-hint`, for example) still loads here.
///
/// `allowed_tools` is parsed and exposed, not enforced: activating a skill
/// never changes the tools a consumer's [`grizzly_agent_core::ToolSet`]
/// advertises. Enforcing it is the consumer's policy to implement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// 1–64 characters, lowercase alphanumerics and single hyphens, no
    /// leading, trailing, or consecutive hyphen; equal to the skill's
    /// directory name.
    pub name: String,
    /// 1–1024 characters describing what the skill does and when to use it.
    pub description: String,
    /// The license applied to the skill, if declared.
    pub license: Option<String>,
    /// Environment requirements (intended product, packages, network
    /// access), at most 500 characters, if declared.
    pub compatibility: Option<String>,
    /// A flat string-to-string map of additional, consumer-defined
    /// properties.
    pub metadata: BTreeMap<String, String>,
    /// Tool names the skill is pre-approved to use, accepted from the
    /// frontmatter as either a space-delimited string or a YAML list of
    /// strings.
    pub allowed_tools: Vec<String>,
    /// Frontmatter keys outside the six the spec defines, with their raw
    /// YAML values.
    pub extra: BTreeMap<String, Yaml>,
}

/// Why a `SKILL.md` file's frontmatter failed to parse or validate.
///
/// Every variant names the skill's directory, so an error is legible on its
/// own without a caller re-attaching file context.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SkillError {
    /// The file's frontmatter could not be split out or parsed as YAML.
    #[error("skill `{directory}`: invalid frontmatter: {source}")]
    Frontmatter {
        /// The skill's directory name.
        directory: String,
        /// The underlying parse failure.
        #[source]
        source: grizzly_agent_prompts::FrontmatterError,
    },
    /// A required field is absent.
    #[error("skill `{directory}`: missing required field `{field}`")]
    MissingField {
        /// The skill's directory name.
        directory: String,
        /// The field that is missing.
        field: &'static str,
    },
    /// A field is present but does not satisfy the spec's constraints.
    #[error("skill `{directory}`: invalid field `{field}`: {reason}")]
    InvalidField {
        /// The skill's directory name.
        directory: String,
        /// The field that failed validation.
        field: &'static str,
        /// What was wrong with it.
        reason: String,
    },
}

const KNOWN_FIELDS: [&str; 6] = [
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];

/// Parses and validates one `SKILL.md` file's content.
///
/// `directory_name` is the name of the directory the file lives in — used
/// only to check that `name` matches it, per the spec.
///
/// # Errors
/// Returns [`SkillError::Frontmatter`] if the file has no `---`-delimited
/// frontmatter or the frontmatter is not valid YAML, [`SkillError::MissingField`]
/// if `name` or `description` is absent, or [`SkillError::InvalidField`] if
/// any field violates the spec's constraints.
pub fn parse_skill(content: &str, directory_name: &str) -> Result<Skill, SkillError> {
    let directory = directory_name.to_owned();
    let frontmatter = grizzly_agent_prompts::parse_frontmatter(content).map_err(|source| {
        SkillError::Frontmatter {
            directory: directory.clone(),
            source,
        }
    })?;
    let hash = frontmatter
        .yaml
        .as_hash()
        .ok_or_else(|| SkillError::InvalidField {
            directory: directory.clone(),
            field: "frontmatter",
            reason: "must be a YAML mapping".to_owned(),
        })?;

    let name = required_string(hash, "name", &directory)?;
    validate_name(&name, directory_name, &directory)?;

    let description = required_string(hash, "description", &directory)?;
    validate_length(&description, "description", 1, 1024, &directory)?;

    let license = optional_string(hash, "license", &directory)?;

    let compatibility = optional_string(hash, "compatibility", &directory)?;
    if let Some(value) = &compatibility {
        validate_length(value, "compatibility", 1, 500, &directory)?;
    }

    let metadata = parse_metadata(hash, &directory)?;
    let allowed_tools = parse_allowed_tools(hash, &directory)?;
    let extra = collect_extra(hash);

    Ok(Skill {
        name,
        description,
        license,
        compatibility,
        metadata,
        allowed_tools,
        extra,
    })
}

fn get<'a>(hash: &'a Hash, key: &str) -> Option<&'a Yaml> {
    hash.get(&Yaml::String(key.to_owned()))
}

fn invalid_field(directory: &str, field: &'static str, reason: impl Into<String>) -> SkillError {
    SkillError::InvalidField {
        directory: directory.to_owned(),
        field,
        reason: reason.into(),
    }
}

fn required_string(
    hash: &Hash,
    field: &'static str,
    directory: &str,
) -> Result<String, SkillError> {
    match get(hash, field) {
        None | Some(Yaml::Null) => Err(SkillError::MissingField {
            directory: directory.to_owned(),
            field,
        }),
        Some(Yaml::String(value)) => Ok(value.clone()),
        Some(_) => Err(invalid_field(directory, field, "must be a string")),
    }
}

fn optional_string(
    hash: &Hash,
    field: &'static str,
    directory: &str,
) -> Result<Option<String>, SkillError> {
    match get(hash, field) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid_field(directory, field, "must be a string")),
    }
}

fn validate_length(
    value: &str,
    field: &'static str,
    min: usize,
    max: usize,
    directory: &str,
) -> Result<(), SkillError> {
    let len = value.chars().count();
    if len < min || len > max {
        return Err(invalid_field(
            directory,
            field,
            format!("must be {min}-{max} characters, got {len}"),
        ));
    }
    Ok(())
}

fn validate_name(name: &str, directory_name: &str, directory: &str) -> Result<(), SkillError> {
    validate_length(name, "name", 1, 64, directory)?;
    if !name
        .chars()
        .all(|c| c == '-' || (c.is_alphanumeric() && !c.is_uppercase()))
    {
        return Err(invalid_field(
            directory,
            "name",
            "must contain only lowercase alphanumeric characters and hyphens",
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(invalid_field(
            directory,
            "name",
            "must not start or end with a hyphen",
        ));
    }
    if name.contains("--") {
        return Err(invalid_field(
            directory,
            "name",
            "must not contain consecutive hyphens",
        ));
    }
    if name != directory_name {
        return Err(invalid_field(
            directory,
            "name",
            format!("must match its directory name `{directory_name}`"),
        ));
    }
    Ok(())
}

fn parse_metadata(hash: &Hash, directory: &str) -> Result<BTreeMap<String, String>, SkillError> {
    let entries = match get(hash, "metadata") {
        None | Some(Yaml::Null) => return Ok(BTreeMap::new()),
        Some(Yaml::Hash(entries)) => entries,
        Some(_) => {
            return Err(invalid_field(
                directory,
                "metadata",
                "must be a mapping of string keys to string values",
            ));
        }
    };

    let mut metadata = BTreeMap::new();
    for (key, value) in entries {
        let (Some(key), Some(value)) = (key.as_str(), value.as_str()) else {
            return Err(invalid_field(
                directory,
                "metadata",
                "must be a flat mapping of string keys to string values",
            ));
        };
        metadata.insert(key.to_owned(), value.to_owned());
    }
    Ok(metadata)
}

fn parse_allowed_tools(hash: &Hash, directory: &str) -> Result<Vec<String>, SkillError> {
    match get(hash, "allowed-tools") {
        None | Some(Yaml::Null) => Ok(Vec::new()),
        Some(Yaml::String(value)) => Ok(value.split_whitespace().map(str::to_owned).collect()),
        Some(Yaml::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().map(str::to_owned).ok_or_else(|| {
                    invalid_field(directory, "allowed-tools", "list items must be strings")
                })
            })
            .collect(),
        Some(_) => Err(invalid_field(
            directory,
            "allowed-tools",
            "must be a space-delimited string or a list of strings",
        )),
    }
}

fn collect_extra(hash: &Hash) -> BTreeMap<String, Yaml> {
    hash.iter()
        .filter_map(|(key, value)| {
            let key = key.as_str()?;
            (!KNOWN_FIELDS.contains(&key)).then(|| (key.to_owned(), value.clone()))
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/format.rs"]
mod tests;
