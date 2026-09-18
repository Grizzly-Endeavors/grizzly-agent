//! Per-file parsing: split frontmatter from body via the crate's default
//! face, hand-map the frontmatter fields from the parsed YAML document,
//! normalize and check the body, tokenize placeholders. No cross-file logic
//! and no semantic validation beyond what a single file can decide on its
//! own — that lives in [`super::validate`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yaml_rust2::Yaml;
use yaml_rust2::yaml::Hash;

use crate::frontmatter::{FrontmatterError, parse_frontmatter};

use super::error::PromptError;
use super::model::BodySegment;

/// A single file's raw frontmatter fields plus its normalized body. Fields are
/// `Option` so the validator can report a precise rule (missing id vs bad id)
/// rather than an opaque parse failure.
#[derive(Debug)]
pub(crate) struct RawFile {
    pub(crate) rel_path: PathBuf,
    pub(crate) id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) tool_schema: Option<Hash>,
    pub(crate) params_from: Option<String>,
    pub(crate) annotations: Option<RawAnnotations>,
    /// The body with a single trailing newline trimmed. Not yet tokenized.
    pub(crate) body: String,
}

#[derive(Debug, Default)]
pub(crate) struct RawAnnotations {
    pub(crate) sent_when: Option<String>,
    pub(crate) used_by: Option<Vec<RawUsedBy>>,
    pub(crate) variables: Option<BTreeMap<String, RawVariable>>,
    pub(crate) reasoning: Option<Vec<String>>,
}

#[derive(Debug)]
pub(crate) struct RawUsedBy {
    pub(crate) file: Option<String>,
    pub(crate) function: Option<String>,
}

#[derive(Debug)]
pub(crate) struct RawVariable {
    pub(crate) source: Option<String>,
    pub(crate) contents: Option<String>,
}

#[derive(Debug)]
pub(crate) struct RawParam {
    pub(crate) ty: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) optional: bool,
    pub(crate) values: Option<Vec<String>>,
}

const FRONTMATTER_KEYS: &[&str] = &[
    "id",
    "type",
    "name",
    "tool_schema",
    "params_from",
    "annotations",
];
const ANNOTATIONS_KEYS: &[&str] = &["sent_when", "used_by", "variables", "reasoning"];
const USED_BY_KEYS: &[&str] = &["file", "function"];
const VARIABLE_KEYS: &[&str] = &["source", "contents"];
const PARAM_KEYS: &[&str] = &["type", "description", "optional", "values"];

/// Parse one file's text into its raw frontmatter and normalized body.
///
/// # Errors
///
/// Returns [`PromptError::Frontmatter`] if the delimiters are missing, the
/// YAML does not parse, a field has the wrong shape, or an unknown key is
/// present, or [`PromptError::BodyWhitespace`] if the body begins or ends with
/// whitespace.
pub(crate) fn parse_file(rel_path: &Path, content: &str) -> Result<RawFile, PromptError> {
    let frontmatter = parse_frontmatter(content).map_err(|err| PromptError::Frontmatter {
        path: rel_path.to_path_buf(),
        message: match err {
            FrontmatterError::MissingDelimiters => err.to_string(),
            FrontmatterError::InvalidYaml(message) => message,
        },
    })?;

    let hash = frontmatter
        .yaml
        .as_hash()
        .ok_or_else(|| PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: "frontmatter must be a YAML mapping".to_owned(),
        })?;
    deny_unknown_keys(rel_path, hash, FRONTMATTER_KEYS, "frontmatter")?;

    let id = get_string(rel_path, hash, "id", "frontmatter")?;
    let kind = get_string(rel_path, hash, "type", "frontmatter")?;
    let name = get_string(rel_path, hash, "name", "frontmatter")?;
    let tool_schema = get_hash(rel_path, hash, "tool_schema", "frontmatter")?.cloned();
    let params_from = get_string(rel_path, hash, "params_from", "frontmatter")?;
    let annotations = match get(hash, "annotations") {
        None | Some(Yaml::Null) => None,
        Some(Yaml::Hash(ann_hash)) => Some(parse_raw_annotations(rel_path, ann_hash)?),
        Some(_) => {
            return Err(PromptError::Frontmatter {
                path: rel_path.to_path_buf(),
                message: "'annotations' must be a mapping".to_owned(),
            });
        }
    };

    let body = normalize_body(frontmatter.body);
    if body.starts_with(char::is_whitespace) || body.ends_with(char::is_whitespace) {
        return Err(PromptError::BodyWhitespace {
            path: rel_path.to_path_buf(),
        });
    }

    Ok(RawFile {
        rel_path: rel_path.to_path_buf(),
        id,
        kind,
        name,
        tool_schema,
        params_from,
        annotations,
        body,
    })
}

fn parse_raw_annotations(rel_path: &Path, hash: &Hash) -> Result<RawAnnotations, PromptError> {
    deny_unknown_keys(rel_path, hash, ANNOTATIONS_KEYS, "annotations")?;
    let sent_when = get_string(rel_path, hash, "sent_when", "annotations")?;
    let used_by = parse_used_by(rel_path, hash)?;
    let variables = parse_variables(rel_path, hash)?;
    let reasoning = get_string_list(rel_path, hash, "reasoning", "annotations")?;
    Ok(RawAnnotations {
        sent_when,
        used_by,
        variables,
        reasoning,
    })
}

fn parse_used_by(rel_path: &Path, hash: &Hash) -> Result<Option<Vec<RawUsedBy>>, PromptError> {
    match get(hash, "used_by") {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let entry_hash = item.as_hash().ok_or_else(|| PromptError::Frontmatter {
                    path: rel_path.to_path_buf(),
                    message: "'annotations.used_by' entries must be mappings".to_owned(),
                })?;
                deny_unknown_keys(
                    rel_path,
                    entry_hash,
                    USED_BY_KEYS,
                    "annotations.used_by entry",
                )?;
                let file = get_string(rel_path, entry_hash, "file", "annotations.used_by entry")?;
                let function = get_string(
                    rel_path,
                    entry_hash,
                    "function",
                    "annotations.used_by entry",
                )?;
                out.push(RawUsedBy { file, function });
            }
            Ok(Some(out))
        }
        Some(_) => Err(PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: "'annotations.used_by' must be a list".to_owned(),
        }),
    }
}

fn parse_variables(
    rel_path: &Path,
    hash: &Hash,
) -> Result<Option<BTreeMap<String, RawVariable>>, PromptError> {
    match get(hash, "variables") {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::Hash(vars_hash)) => {
            let mut out = BTreeMap::new();
            for (key, value) in vars_hash {
                let name = key.as_str().ok_or_else(|| PromptError::Frontmatter {
                    path: rel_path.to_path_buf(),
                    message: "'annotations.variables' keys must be strings".to_owned(),
                })?;
                let entry_hash = value.as_hash().ok_or_else(|| PromptError::Frontmatter {
                    path: rel_path.to_path_buf(),
                    message: format!("'annotations.variables.{name}' must be a mapping"),
                })?;
                let context = format!("annotations.variables.{name}");
                deny_unknown_keys(rel_path, entry_hash, VARIABLE_KEYS, &context)?;
                let source = get_string(rel_path, entry_hash, "source", &context)?;
                let contents = get_string(rel_path, entry_hash, "contents", &context)?;
                out.insert(name.to_owned(), RawVariable { source, contents });
            }
            Ok(Some(out))
        }
        Some(_) => Err(PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: "'annotations.variables' must be a mapping".to_owned(),
        }),
    }
}

/// Hand-map one `tool_schema` entry's value into a [`RawParam`]. Shared by
/// [`super::validate`], which owns iteration order and per-parameter
/// validation.
pub(crate) fn parse_raw_param(rel_path: &Path, value: &Yaml) -> Result<RawParam, PromptError> {
    let hash = value.as_hash().ok_or_else(|| PromptError::Frontmatter {
        path: rel_path.to_path_buf(),
        message: "tool_schema parameter must be a mapping".to_owned(),
    })?;
    deny_unknown_keys(rel_path, hash, PARAM_KEYS, "tool_schema parameter")?;
    let ty = get_string(rel_path, hash, "type", "tool_schema parameter")?;
    let description = get_string(rel_path, hash, "description", "tool_schema parameter")?;
    let optional = get_bool(rel_path, hash, "optional", "tool_schema parameter")?;
    let values = get_string_list(rel_path, hash, "values", "tool_schema parameter")?;
    Ok(RawParam {
        ty,
        description,
        optional,
        values,
    })
}

fn get<'y>(hash: &'y Hash, key: &str) -> Option<&'y Yaml> {
    hash.get(&Yaml::String(key.to_owned()))
}

fn deny_unknown_keys(
    rel_path: &Path,
    hash: &Hash,
    allowed: &[&str],
    context: &str,
) -> Result<(), PromptError> {
    for key in hash.keys() {
        let key_str = key.as_str().ok_or_else(|| PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: format!("{context} has a non-string key"),
        })?;
        if !allowed.contains(&key_str) {
            return Err(PromptError::Frontmatter {
                path: rel_path.to_path_buf(),
                message: format!("unknown key '{key_str}' in {context}"),
            });
        }
    }
    Ok(())
}

fn get_string(
    rel_path: &Path,
    hash: &Hash,
    key: &str,
    context: &str,
) -> Result<Option<String>, PromptError> {
    match get(hash, key) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: format!("'{key}' in {context} must be a string"),
        }),
    }
}

fn get_bool(rel_path: &Path, hash: &Hash, key: &str, context: &str) -> Result<bool, PromptError> {
    match get(hash, key) {
        None | Some(Yaml::Null) => Ok(false),
        Some(Yaml::Boolean(b)) => Ok(*b),
        Some(_) => Err(PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: format!("'{key}' in {context} must be a boolean"),
        }),
    }
}

fn get_hash<'y>(
    rel_path: &Path,
    hash: &'y Hash,
    key: &str,
    context: &str,
) -> Result<Option<&'y Hash>, PromptError> {
    match get(hash, key) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::Hash(h)) => Ok(Some(h)),
        Some(_) => Err(PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: format!("'{key}' in {context} must be a mapping"),
        }),
    }
}

fn get_string_list(
    rel_path: &Path,
    hash: &Hash,
    key: &str,
    context: &str,
) -> Result<Option<Vec<String>>, PromptError> {
    match get(hash, key) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| PromptError::Frontmatter {
                        path: rel_path.to_path_buf(),
                        message: format!("'{key}' in {context} must be a list of strings"),
                    })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => Err(PromptError::Frontmatter {
            path: rel_path.to_path_buf(),
            message: format!("'{key}' in {context} must be a list"),
        }),
    }
}

/// Trim exactly one trailing newline (`\r\n` or `\n`), per the format rule.
fn normalize_body(body: &str) -> String {
    body.strip_suffix("\r\n")
        .or_else(|| body.strip_suffix('\n'))
        .unwrap_or(body)
        .to_owned()
}

/// Tokenize a body into literal and `{{placeholder}}` segments.
///
/// # Errors
///
/// Returns [`TokenizeError::Unterminated`] if a `{{` has no closing `}}`.
pub(crate) fn tokenize_body(body: &str) -> Result<Vec<BodySegment>, TokenizeError> {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next();
            if !literal.is_empty() {
                segments.push(BodySegment::Literal(std::mem::take(&mut literal)));
            }
            segments.push(BodySegment::Placeholder(read_placeholder(&mut chars)?));
        } else {
            literal.push(c);
        }
    }
    if !literal.is_empty() {
        segments.push(BodySegment::Literal(literal));
    }
    Ok(segments)
}

/// Read placeholder characters up to and including the closing `}}`.
fn read_placeholder(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, TokenizeError> {
    let mut name = String::new();
    loop {
        match chars.next() {
            Some('}') if chars.peek() == Some(&'}') => {
                chars.next();
                return Ok(name);
            }
            Some(ch) => name.push(ch),
            None => return Err(TokenizeError::Unterminated),
        }
    }
}

/// Why body tokenization failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenizeError {
    Unterminated,
}

#[cfg(test)]
#[path = "tests/parse.rs"]
mod tests;
