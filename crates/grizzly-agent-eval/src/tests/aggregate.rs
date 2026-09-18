use std::time::Duration;

use grizzly_agent_core::Usage;

use super::*;

fn verdict_with(category: VerdictCategory) -> Verdict {
    match category {
        VerdictCategory::Pass => Verdict::pass("ok"),
        VerdictCategory::Wrong => Verdict::wrong("no"),
        VerdictCategory::Unparseable => Verdict::unparseable("garbled"),
        VerdictCategory::Unavailable => Verdict::unavailable("timed out"),
        VerdictCategory::Failed => Verdict::failed("case bug"),
    }
}

#[test]
fn aggregate_counts_every_category() {
    let meta = CaseMeta::new("case");
    let verdicts = vec![
        verdict_with(VerdictCategory::Pass),
        verdict_with(VerdictCategory::Pass),
        verdict_with(VerdictCategory::Wrong),
        verdict_with(VerdictCategory::Unparseable),
        verdict_with(VerdictCategory::Unavailable),
        verdict_with(VerdictCategory::Failed),
    ];

    let aggregate = CaseAggregate::aggregate(&meta, &verdicts);

    assert_eq!(aggregate.repeats, 6, "repeats must count every verdict");
    assert_eq!(aggregate.passes, 2, "passes must count only Pass verdicts");
    assert_eq!(aggregate.wrong, 1, "wrong must count only Wrong verdicts");
    assert_eq!(
        aggregate.unparseable, 1,
        "unparseable must count only Unparseable verdicts"
    );
    assert_eq!(
        aggregate.unavailable, 1,
        "unavailable must count only Unavailable verdicts"
    );
    assert_eq!(
        aggregate.failed, 1,
        "failed must count only Failed verdicts"
    );
    assert!(
        (aggregate.pass_rate - (2.0 / 6.0)).abs() < f64::EPSILON,
        "pass_rate must be passes divided by repeats"
    );
}

#[test]
fn met_threshold_compares_pass_rate_to_threshold() {
    let meta = CaseMeta {
        min_pass_rate: Some(0.5),
        ..CaseMeta::new("case")
    };
    let verdicts = vec![
        verdict_with(VerdictCategory::Pass),
        verdict_with(VerdictCategory::Wrong),
    ];

    let aggregate = CaseAggregate::aggregate(&meta, &verdicts);
    assert!(
        aggregate.met_threshold,
        "a 0.5 pass rate must meet a 0.5 threshold"
    );

    let stricter = CaseMeta {
        min_pass_rate: Some(0.75),
        ..CaseMeta::new("case")
    };
    let stricter_aggregate = CaseAggregate::aggregate(&stricter, &verdicts);
    assert!(
        !stricter_aggregate.met_threshold,
        "a 0.5 pass rate must not meet a 0.75 threshold"
    );
}

#[test]
fn a_canary_needs_every_repeat_to_pass() {
    let meta = CaseMeta {
        canary: true,
        ..CaseMeta::new("case")
    };
    let all_pass = vec![
        verdict_with(VerdictCategory::Pass),
        verdict_with(VerdictCategory::Pass),
    ];
    assert!(
        CaseAggregate::aggregate(&meta, &all_pass).met_threshold,
        "a canary with every repeat passing must meet its threshold"
    );

    let one_miss = vec![
        verdict_with(VerdictCategory::Pass),
        verdict_with(VerdictCategory::Wrong),
    ];
    assert!(
        !CaseAggregate::aggregate(&meta, &one_miss).met_threshold,
        "a canary with even one miss must not meet its threshold"
    );
}

#[test]
fn zero_repeats_never_meets_its_threshold() {
    let meta = CaseMeta {
        min_pass_rate: Some(0.0),
        ..CaseMeta::new("case")
    };
    let aggregate = CaseAggregate::aggregate(&meta, &[]);
    assert!(
        aggregate.pass_rate.abs() < f64::EPSILON,
        "an empty run's pass rate must be 0.0"
    );
    assert!(
        !aggregate.met_threshold,
        "an empty run must never meet its threshold, even one of 0.0"
    );
}

#[test]
fn latency_and_usage_summaries_average_only_reporting_repeats() {
    let meta = CaseMeta::new("case");
    let verdicts = vec![
        Verdict::pass("ok")
            .with_latency(Duration::from_millis(100))
            .with_usage(Usage {
                input_tokens: Some(10),
                output_tokens: Some(20),
            }),
        Verdict::pass("ok").with_latency(Duration::from_millis(200)),
        Verdict::wrong("no"),
    ];

    let aggregate = CaseAggregate::aggregate(&meta, &verdicts);

    assert_eq!(
        aggregate.mean_latency,
        Some(Duration::from_millis(150)),
        "mean_latency must average only the repeats that reported one"
    );
    assert_eq!(
        aggregate.mean_input_tokens,
        Some(10),
        "mean_input_tokens must average only the repeats that reported usage"
    );
    assert_eq!(
        aggregate.mean_output_tokens,
        Some(20),
        "mean_output_tokens must average only the repeats that reported usage"
    );
}

#[test]
fn summaries_are_none_when_no_repeat_reported_them() {
    let meta = CaseMeta::new("case");
    let verdicts = vec![Verdict::pass("ok"), Verdict::wrong("no")];

    let aggregate = CaseAggregate::aggregate(&meta, &verdicts);

    assert_eq!(
        aggregate.mean_latency, None,
        "mean_latency must be None when no repeat reported a latency"
    );
    assert_eq!(
        aggregate.mean_input_tokens, None,
        "mean_input_tokens must be None when no repeat reported usage"
    );
}
