use std::time::Duration;

use grizzly_agent_core::Usage;

use super::*;

#[test]
fn each_constructor_sets_its_category() {
    assert_eq!(
        Verdict::pass("ok").category,
        VerdictCategory::Pass,
        "pass must build a Pass verdict"
    );
    assert_eq!(
        Verdict::wrong("no").category,
        VerdictCategory::Wrong,
        "wrong must build a Wrong verdict"
    );
    assert_eq!(
        Verdict::unparseable("garbled").category,
        VerdictCategory::Unparseable,
        "unparseable must build an Unparseable verdict"
    );
    assert_eq!(
        Verdict::unavailable("timed out").category,
        VerdictCategory::Unavailable,
        "unavailable must build an Unavailable verdict"
    );
    assert_eq!(
        Verdict::failed("case bug").category,
        VerdictCategory::Failed,
        "failed must build a Failed verdict"
    );
}

#[test]
fn only_pass_counts_as_passed() {
    assert!(
        Verdict::pass("ok").passed(),
        "a Pass verdict must count as passed"
    );
    assert!(
        !Verdict::wrong("no").passed(),
        "a Wrong verdict must not count as passed"
    );
    assert!(
        !Verdict::unparseable("garbled").passed(),
        "an Unparseable verdict must not count as passed"
    );
    assert!(
        !Verdict::unavailable("timed out").passed(),
        "an Unavailable verdict must not count as passed"
    );
    assert!(
        !Verdict::failed("case bug").passed(),
        "a Failed verdict must not count as passed"
    );
}

#[test]
fn with_latency_and_usage_attach_without_changing_the_category() {
    let usage = Usage {
        input_tokens: Some(10),
        output_tokens: Some(20),
    };
    let verdict = Verdict::pass("ok")
        .with_latency(Duration::from_millis(150))
        .with_usage(usage);

    assert_eq!(
        verdict.latency,
        Some(Duration::from_millis(150)),
        "with_latency must set the latency field"
    );
    assert_eq!(
        verdict.usage,
        Some(usage),
        "with_usage must set the usage field"
    );
    assert!(
        verdict.passed(),
        "attaching latency and usage must not change the category"
    );
}

#[test]
fn verdict_round_trips_through_json() {
    let verdict = Verdict::wrong("answered b, expected a")
        .with_latency(Duration::from_millis(42))
        .with_usage(Usage {
            input_tokens: Some(5),
            output_tokens: None,
        });

    let json = serde_json::to_string(&verdict).expect("a verdict must serialize");
    let restored: Verdict =
        serde_json::from_str(&json).expect("a serialized verdict must deserialize");
    assert_eq!(
        restored, verdict,
        "a verdict must round-trip through JSON unchanged"
    );
}
