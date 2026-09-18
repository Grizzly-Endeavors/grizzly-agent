use super::*;

#[test]
fn no_params_accepts_an_empty_object() {
    let result: Result<NoParams, _> = serde_json::from_str("{}");
    assert!(
        result.is_ok(),
        "a model sends `{{}}` for a no-argument tool call, which must parse"
    );
}

#[test]
fn no_params_rejects_unknown_fields() {
    let result: Result<NoParams, _> = serde_json::from_str(r#"{"unexpected": "value"}"#);
    assert!(
        result.is_err(),
        "unexpected arguments to a no-parameter tool must be rejected, not silently ignored"
    );
}

#[test]
fn tool_spec_holds_both_borrowed_and_owned_strings() {
    let borrowed = ToolSpec {
        name: Cow::Borrowed("static_tool"),
        description: Cow::Borrowed("a generated tool"),
        parameters: serde_json::json!({"type": "object"}),
    };
    let owned = ToolSpec {
        name: Cow::Owned(format!("runtime_{}", "tool")),
        description: Cow::Owned("built at runtime".to_owned()),
        parameters: serde_json::json!({"type": "object"}),
    };

    assert_eq!(borrowed.name.as_ref(), "static_tool");
    assert_eq!(owned.name.as_ref(), "runtime_tool");
}
