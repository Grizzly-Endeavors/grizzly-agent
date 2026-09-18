//! Split a Markdown file into YAML frontmatter and body, and parse the
//! frontmatter into a `yaml-rust2` document.
//!
//! No file I/O and no cargo feature gate: this is the crate's default face,
//! shared by prompt codegen and by `grizzly-agent-skills`' `SKILL.md`
//! parsing, since both formats are Markdown with `---`-delimited YAML
//! frontmatter and only this split-and-parse step is common between them —
//! their frontmatter schemas have nothing else in common.

use std::fmt;

use yaml_rust2::{Yaml, YamlLoader, yaml::Hash};

/// One file's YAML frontmatter, parsed as a single document, and its raw
/// (un-normalized) body text.
#[derive(Debug, Clone)]
pub struct Frontmatter<'a> {
    /// The frontmatter, parsed as a YAML document — a mapping for every
    /// valid prompt or skill file. Callers hand-map the fields they expect.
    pub yaml: Yaml,
    /// The body text between the closing delimiter and the end of the file,
    /// exactly as written. Trailing-newline trimming and whitespace rules are
    /// the caller's, since they differ between prompts and skills.
    pub body: &'a str,
}

/// Why a file's frontmatter could not be split out or parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontmatterError {
    /// The opening or closing `---` delimiter is missing.
    MissingDelimiters,
    /// The frontmatter is present but is not valid YAML.
    InvalidYaml(String),
}

impl fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDelimiters => write!(f, "missing '---' frontmatter delimiters"),
            Self::InvalidYaml(message) => write!(f, "invalid frontmatter: {message}"),
        }
    }
}

impl std::error::Error for FrontmatterError {}

/// Split `content` into its YAML frontmatter and body and parse the
/// frontmatter into a single YAML document.
///
/// An empty or absent frontmatter document parses to an empty [`Yaml::Hash`],
/// so callers can look up fields without a preliminary null check.
///
/// # Errors
///
/// Returns [`FrontmatterError::MissingDelimiters`] if the opening or closing
/// `---` delimiter is absent, or [`FrontmatterError::InvalidYaml`] if the
/// frontmatter text is not valid YAML.
pub fn parse_frontmatter(content: &str) -> Result<Frontmatter<'_>, FrontmatterError> {
    let (yaml_text, body) =
        split_frontmatter(content).ok_or(FrontmatterError::MissingDelimiters)?;
    let docs = YamlLoader::load_from_str(yaml_text)
        .map_err(|err| FrontmatterError::InvalidYaml(err.to_string()))?;
    let yaml = docs
        .into_iter()
        .next()
        .unwrap_or_else(|| Yaml::Hash(Hash::default()));
    Ok(Frontmatter { yaml, body })
}

/// Split `---`-delimited frontmatter from the body. Returns the YAML text and
/// the raw (un-trimmed) body, or `None` if the opening or closing delimiter is
/// absent.
#[must_use]
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))?;
    if let Some(split) = rest.split_once("\n---\n") {
        return Some(split);
    }
    if let Some(split) = rest.split_once("\n---\r\n") {
        return Some(split);
    }
    rest.strip_suffix("\n---").map(|yaml| (yaml, ""))
}

#[cfg(test)]
#[path = "tests/frontmatter.rs"]
mod tests;
