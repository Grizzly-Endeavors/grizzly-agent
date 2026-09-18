use super::*;

#[test]
fn passed_and_failed_constructors_set_the_flag() {
    let ok = CheckResult::passed("lint", "exit 0");
    assert!(ok.passed, "passed must set passed to true");
    assert_eq!(ok.name, "lint", "passed must keep the given name");
    assert_eq!(ok.detail, "exit 0", "passed must keep the given detail");

    let bad = CheckResult::failed("lint", "exit 1: unused variable");
    assert!(!bad.passed, "failed must set passed to false");
}

#[test]
fn check_result_round_trips_through_json() {
    let result = CheckResult::failed("tests", "2 failed, 8 passed");
    let json = serde_json::to_string(&result).expect("a check result must serialize");
    let restored: CheckResult =
        serde_json::from_str(&json).expect("a serialized check result must deserialize");
    assert_eq!(
        restored, result,
        "a check result must round-trip through JSON unchanged"
    );
}
