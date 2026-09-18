use super::*;

#[test]
fn splits_frontmatter_and_body() {
    let (yaml, body) = split_frontmatter("---\nid: Greeting\n---\nHello.").unwrap();
    assert_eq!(yaml, "id: Greeting");
    assert_eq!(body, "Hello.");
}

#[test]
fn splits_crlf_delimiters() {
    // Only the `\r\n` at each delimiter boundary is consumed; a mid-content
    // `\r` (from the CRLF line ending before the closing delimiter) survives
    // into the yaml text, same as the un-normalized body does.
    let (yaml, body) = split_frontmatter("---\r\nid: Greeting\r\n---\r\nHello.").unwrap();
    assert_eq!(yaml, "id: Greeting\r");
    assert_eq!(body, "Hello.");
}

#[test]
fn splits_frontmatter_with_no_trailing_body() {
    let (yaml, body) = split_frontmatter("---\nid: Greeting\n---").unwrap();
    assert_eq!(yaml, "id: Greeting");
    assert_eq!(body, "");
}

#[test]
fn missing_opening_delimiter_is_none() {
    assert!(split_frontmatter("id: Greeting\n---\nHello.").is_none());
}

#[test]
fn missing_closing_delimiter_is_none() {
    assert!(split_frontmatter("---\nid: Greeting\nHello.").is_none());
}

#[test]
fn parses_a_mapping_document() {
    let frontmatter = parse_frontmatter("---\nid: Greeting\ntype: prompt\n---\nHello.").unwrap();
    let hash = frontmatter.yaml.as_hash().unwrap();
    assert_eq!(
        hash.get(&Yaml::String("id".to_owned())),
        Some(&Yaml::String("Greeting".to_owned()))
    );
    assert_eq!(frontmatter.body, "Hello.");
}

#[test]
fn preserves_mapping_key_order() {
    let frontmatter = parse_frontmatter("---\nb: 2\na: 1\nc: 3\n---\n").unwrap();
    let hash = frontmatter.yaml.as_hash().unwrap();
    let keys: Vec<&str> = hash.keys().filter_map(Yaml::as_str).collect();
    assert_eq!(
        keys,
        ["b", "a", "c"],
        "yaml-rust2 preserves insertion order"
    );
}

#[test]
fn empty_frontmatter_parses_to_an_empty_hash() {
    let frontmatter = parse_frontmatter("---\n\n---\nBody").unwrap();
    assert_eq!(frontmatter.yaml.as_hash().map(Hash::is_empty), Some(true));
    assert_eq!(frontmatter.body, "Body");
}

#[test]
fn missing_delimiters_is_an_error() {
    let err = parse_frontmatter("no frontmatter here").unwrap_err();
    assert_eq!(err, FrontmatterError::MissingDelimiters);
}

#[test]
fn invalid_yaml_is_an_error() {
    let err = parse_frontmatter("---\nid: [unterminated\n---\nBody.").unwrap_err();
    assert!(
        matches!(err, FrontmatterError::InvalidYaml(_)),
        "got {err:?}"
    );
}
