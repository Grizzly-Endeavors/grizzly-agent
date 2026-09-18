use super::*;

#[test]
fn effective_repeats_prefers_its_own_count_over_the_suite_default() {
    let with_override = CaseMeta {
        repeats: Some(5),
        ..CaseMeta::new("case")
    };
    assert_eq!(
        with_override.effective_repeats(3),
        5,
        "a case's own repeats must win over the suite default"
    );

    let without_override = CaseMeta::new("case");
    assert_eq!(
        without_override.effective_repeats(3),
        3,
        "a case with no repeats set must fall back to the suite default"
    );
}

#[test]
fn threshold_falls_back_to_the_default_pass_rate() {
    let meta = CaseMeta::new("case");
    assert!(
        (meta.threshold() - DEFAULT_MIN_PASS_RATE).abs() < f64::EPSILON,
        "a case with no min_pass_rate must use the default"
    );
}

#[test]
fn threshold_uses_its_own_pass_rate_when_set() {
    let meta = CaseMeta {
        min_pass_rate: Some(0.5),
        ..CaseMeta::new("case")
    };
    assert!(
        (meta.threshold() - 0.5).abs() < f64::EPSILON,
        "a case's own min_pass_rate must override the default"
    );
}

#[test]
fn a_canary_thresholds_at_one_regardless_of_min_pass_rate() {
    let meta = CaseMeta {
        min_pass_rate: Some(0.1),
        canary: true,
        ..CaseMeta::new("case")
    };
    assert!(
        (meta.threshold() - 1.0).abs() < f64::EPSILON,
        "a canary's threshold must always be 1.0, even with a lower min_pass_rate set"
    );
}

#[test]
fn case_meta_deserializes_from_json() {
    let meta: CaseMeta = serde_json::from_str(
        r#"{"name": "case-1", "repeats": 5, "min_pass_rate": 0.9, "canary": true}"#,
    )
    .expect("a fully populated document must deserialize");
    assert_eq!(
        meta,
        CaseMeta {
            name: "case-1".to_owned(),
            repeats: Some(5),
            min_pass_rate: Some(0.9),
            canary: true,
        },
        "every field must deserialize to what the document declared"
    );
}

#[test]
fn case_meta_deserializes_with_only_a_name() {
    let meta: CaseMeta = serde_json::from_str(r#"{"name": "case-1"}"#)
        .expect("a name-only document must deserialize");
    assert_eq!(
        meta,
        CaseMeta::new("case-1"),
        "omitted fields must default to None/false, not fail to parse"
    );
}
