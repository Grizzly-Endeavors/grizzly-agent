//! Table tests for [`super`]: one test per validation rule, plus both
//! accepted `allowed-tools` forms.

use super::*;

/// Wraps `frontmatter` in `---` delimiters with a trivial body, the shape
/// every case below starts from.
fn skill_md(frontmatter: &str) -> String {
    format!("---\n{frontmatter}\n---\n\nDo the thing.\n")
}

fn minimal(name: &str) -> String {
    skill_md(&format!("name: {name}\ndescription: does a thing.\n"))
}

#[test]
fn valid_minimal_skill_parses() {
    let skill = parse_skill(&minimal("do-thing"), "do-thing").expect("must parse");
    assert_eq!(skill.name, "do-thing");
    assert_eq!(skill.description, "does a thing.");
    assert_eq!(skill.license, None);
    assert_eq!(skill.compatibility, None);
    assert!(skill.metadata.is_empty());
    assert!(skill.allowed_tools.is_empty());
    assert!(skill.extra.is_empty());
}

#[test]
fn valid_skill_with_every_optional_field_parses() {
    let content = skill_md(
        "name: full-skill\n\
         description: exercises every optional field.\n\
         license: Apache-2.0\n\
         compatibility: requires git and jq\n\
         metadata:\n  author: example-org\n  version: \"1.0\"\n\
         allowed-tools: \"Bash(git:*) Read\"\n",
    );
    let skill = parse_skill(&content, "full-skill").expect("must parse");
    assert_eq!(skill.license.as_deref(), Some("Apache-2.0"));
    assert_eq!(skill.compatibility.as_deref(), Some("requires git and jq"));
    assert_eq!(
        skill.metadata.get("author"),
        Some(&"example-org".to_owned())
    );
    assert_eq!(skill.metadata.get("version"), Some(&"1.0".to_owned()));
    assert_eq!(skill.allowed_tools, vec!["Bash(git:*)", "Read"]);
}

#[test]
fn missing_name_is_reported() {
    let content = skill_md("description: does a thing.\n");
    let error = parse_skill(&content, "do-thing").expect_err("name is required");
    assert!(matches!(
        error,
        SkillError::MissingField { field: "name", .. }
    ));
}

#[test]
fn missing_description_is_reported() {
    let content = skill_md("name: do-thing\n");
    let error = parse_skill(&content, "do-thing").expect_err("description is required");
    assert!(matches!(
        error,
        SkillError::MissingField {
            field: "description",
            ..
        }
    ));
}

#[test]
fn name_over_64_characters_is_rejected() {
    let long_name = "a".repeat(65);
    let content = minimal(&long_name);
    let error = parse_skill(&content, &long_name).expect_err("64-char limit");
    assert!(matches!(
        error,
        SkillError::InvalidField { field: "name", .. }
    ));
}

#[test]
fn name_with_uppercase_is_rejected() {
    let content = minimal("Do-Thing");
    let error = parse_skill(&content, "Do-Thing").expect_err("must be lowercase");
    assert!(matches!(
        error,
        SkillError::InvalidField { field: "name", .. }
    ));
}

#[test]
fn name_with_leading_hyphen_is_rejected() {
    let content = minimal("-do-thing");
    let error = parse_skill(&content, "-do-thing").expect_err("no leading hyphen");
    assert!(matches!(
        error,
        SkillError::InvalidField { field: "name", .. }
    ));
}

#[test]
fn name_with_trailing_hyphen_is_rejected() {
    let content = minimal("do-thing-");
    let error = parse_skill(&content, "do-thing-").expect_err("no trailing hyphen");
    assert!(matches!(
        error,
        SkillError::InvalidField { field: "name", .. }
    ));
}

#[test]
fn name_with_consecutive_hyphens_is_rejected() {
    let content = minimal("do--thing");
    let error = parse_skill(&content, "do--thing").expect_err("no consecutive hyphens");
    assert!(matches!(
        error,
        SkillError::InvalidField { field: "name", .. }
    ));
}

#[test]
fn name_not_matching_directory_is_rejected() {
    let content = minimal("do-thing");
    let error = parse_skill(&content, "other-directory").expect_err("must match its directory");
    assert!(matches!(
        error,
        SkillError::InvalidField { field: "name", .. }
    ));
}

#[test]
fn empty_description_is_rejected() {
    let content = skill_md("name: do-thing\ndescription: \"\"\n");
    let error = parse_skill(&content, "do-thing").expect_err("description must be non-empty");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "description",
            ..
        }
    ));
}

#[test]
fn description_over_1024_characters_is_rejected() {
    let long_description = "a".repeat(1025);
    let content = skill_md(&format!(
        "name: do-thing\ndescription: {long_description}\n"
    ));
    let error = parse_skill(&content, "do-thing").expect_err("1024-char limit");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "description",
            ..
        }
    ));
}

#[test]
fn compatibility_over_500_characters_is_rejected() {
    let long_compat = "a".repeat(501);
    let content = skill_md(&format!(
        "name: do-thing\ndescription: does a thing.\ncompatibility: {long_compat}\n"
    ));
    let error = parse_skill(&content, "do-thing").expect_err("500-char limit");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "compatibility",
            ..
        }
    ));
}

#[test]
fn empty_compatibility_is_rejected() {
    let content = skill_md("name: do-thing\ndescription: does a thing.\ncompatibility: \"\"\n");
    let error = parse_skill(&content, "do-thing").expect_err("compatibility must be non-empty");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "compatibility",
            ..
        }
    ));
}

#[test]
fn nested_metadata_value_is_rejected() {
    let content =
        skill_md("name: do-thing\ndescription: does a thing.\nmetadata:\n  nested:\n    a: b\n");
    let error = parse_skill(&content, "do-thing").expect_err("metadata must be flat");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "metadata",
            ..
        }
    ));
}

#[test]
fn non_string_metadata_value_is_rejected() {
    let content = skill_md("name: do-thing\ndescription: does a thing.\nmetadata:\n  version: 1\n");
    let error = parse_skill(&content, "do-thing").expect_err("metadata values must be strings");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "metadata",
            ..
        }
    ));
}

#[test]
fn allowed_tools_as_space_delimited_string_parses() {
    let content = skill_md(
        "name: do-thing\ndescription: does a thing.\nallowed-tools: \"Bash(git:*) Read Write\"\n",
    );
    let skill = parse_skill(&content, "do-thing").expect("must parse");
    assert_eq!(skill.allowed_tools, vec!["Bash(git:*)", "Read", "Write"]);
}

#[test]
fn allowed_tools_as_yaml_list_parses() {
    let content = skill_md(
        "name: do-thing\ndescription: does a thing.\nallowed-tools:\n  - Bash(git:*)\n  - Read\n",
    );
    let skill = parse_skill(&content, "do-thing").expect("must parse");
    assert_eq!(skill.allowed_tools, vec!["Bash(git:*)", "Read"]);
}

#[test]
fn allowed_tools_of_the_wrong_type_is_rejected() {
    let content = skill_md("name: do-thing\ndescription: does a thing.\nallowed-tools: 5\n");
    let error =
        parse_skill(&content, "do-thing").expect_err("allowed-tools must be string or list");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "allowed-tools",
            ..
        }
    ));
}

#[test]
fn allowed_tools_list_with_non_string_item_is_rejected() {
    let content =
        skill_md("name: do-thing\ndescription: does a thing.\nallowed-tools:\n  - Read\n  - 5\n");
    let error = parse_skill(&content, "do-thing").expect_err("list items must be strings");
    assert!(matches!(
        error,
        SkillError::InvalidField {
            field: "allowed-tools",
            ..
        }
    ));
}

#[test]
fn unknown_frontmatter_keys_are_preserved() {
    let content = skill_md("name: do-thing\ndescription: does a thing.\nargument-hint: <path>\n");
    let skill = parse_skill(&content, "do-thing").expect("unknown keys must not reject the file");
    assert_eq!(
        skill
            .extra
            .get("argument-hint")
            .and_then(|value| value.as_str()),
        Some("<path>")
    );
}

#[test]
fn missing_frontmatter_delimiters_is_reported() {
    let content = "no frontmatter here";
    let error = parse_skill(content, "do-thing").expect_err("delimiters are required");
    assert!(matches!(error, SkillError::Frontmatter { .. }));
}

#[test]
fn invalid_yaml_is_reported() {
    let content = "---\nname: [unterminated\n---\nbody\n";
    let error = parse_skill(content, "do-thing").expect_err("invalid YAML must fail");
    assert!(matches!(error, SkillError::Frontmatter { .. }));
}
