use jiff::Timestamp;
use uuid::Uuid;

use crate::case::CaseMeta;
use crate::verdict::Verdict;

use super::*;

fn sample_report(cases: Vec<CaseReport>) -> Report {
    Report::new(
        Uuid::now_v7(),
        Timestamp::now(),
        serde_json::json!({"model": "test-model"}),
        cases,
    )
}

fn case_report(name: &str, canary: bool, verdicts: Vec<Verdict>) -> CaseReport {
    let meta = CaseMeta {
        canary,
        ..CaseMeta::new(name)
    };
    let aggregate = CaseAggregate::aggregate(&meta, &verdicts);
    let repeats = verdicts.into_iter().map(RepeatRecord::new).collect();
    CaseReport { aggregate, repeats }
}

#[test]
fn new_report_carries_the_current_schema_version() {
    let report = sample_report(Vec::new());
    assert_eq!(
        report.schema_version, REPORT_SCHEMA_VERSION,
        "Report::new must stamp the current schema version"
    );
}

#[test]
fn suite_result_is_met_only_when_every_case_met_its_threshold() {
    let passing = case_report("a", false, vec![Verdict::pass("ok")]);
    let failing = case_report("b", false, vec![Verdict::wrong("no")]);

    let all_pass = sample_report(vec![passing.clone()]);
    assert!(
        all_pass.suite_result().all_met,
        "a report where every case met its threshold must be all_met"
    );

    let one_fails = sample_report(vec![passing, failing]);
    assert!(
        !one_fails.suite_result().all_met,
        "a report with one case below its threshold must not be all_met"
    );
}

#[test]
fn report_round_trips_through_json() {
    let report = sample_report(vec![case_report(
        "case-1",
        false,
        vec![Verdict::pass("ok"), Verdict::wrong("no")],
    )]);

    let json = serde_json::to_string_pretty(&report).expect("a report must serialize");
    let restored: Report =
        serde_json::from_str(&json).expect("a serialized report must deserialize");
    assert_eq!(
        restored, report,
        "a report must round-trip through JSON unchanged"
    );
}

#[test]
fn render_summary_marks_canaries_and_reports_the_footer() {
    let report = sample_report(vec![
        case_report("watcher-note", true, vec![Verdict::pass("ok")]),
        case_report("judge-deny", false, vec![Verdict::wrong("no")]),
    ]);

    let summary = render_summary(&report);

    assert!(
        summary.contains("watcher-note (canary)"),
        "the summary must mark a canary case: {summary}"
    );
    assert!(
        summary.contains("2 cases, 1 at or above threshold, 1 below"),
        "the summary footer must report the pass/fail split: {summary}"
    );
}

#[test]
fn render_summary_widens_the_case_column_to_the_longest_name() {
    let long_name = "a-case-name-considerably-longer-than-the-minimum-column-width";
    let report = sample_report(vec![case_report(
        long_name,
        false,
        vec![Verdict::pass("ok")],
    )]);

    let summary = render_summary(&report);
    let case_line = summary
        .lines()
        .find(|line| line.contains(long_name))
        .expect("the rendered summary must contain the case's row");
    assert!(
        case_line.starts_with("pass") || case_line.starts_with("FAIL"),
        "the case row must still start with the result column: {case_line:?}"
    );
}
