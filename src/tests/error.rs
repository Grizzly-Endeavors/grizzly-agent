//! Tests for [`super`].

use std::time::Duration;

use super::{ProviderFailure, ToolFailure};

fn status(code: u16) -> ProviderFailure {
    ProviderFailure::Status {
        provider: "test".to_owned(),
        status: code,
        message: "boom".to_owned(),
        retry_after: None,
    }
}

fn transport() -> ProviderFailure {
    ProviderFailure::Transport {
        provider: "test".to_owned(),
        source: Box::new(std::io::Error::from(std::io::ErrorKind::ConnectionReset)),
    }
}

#[test]
fn rate_limits_and_server_errors_are_retryable() {
    for code in [408, 429, 500, 502, 503, 504] {
        assert!(
            status(code).is_retryable(),
            "HTTP {code} should be retryable"
        );
    }
    assert!(
        transport().is_retryable(),
        "transport failures are transient"
    );
}

#[test]
fn client_errors_are_not_retryable() {
    for code in [400, 401, 403, 404, 422] {
        assert!(
            !status(code).is_retryable(),
            "HTTP {code} will fail identically on retry"
        );
    }
}

#[test]
fn a_decode_failure_is_never_retryable() {
    let source = serde_json::from_str::<serde_json::Value>("{not json")
        .expect_err("this input is deliberately malformed");
    let failure = ProviderFailure::Decode {
        provider: "test".to_owned(),
        source,
    };

    assert!(
        !failure.is_retryable(),
        "an identical request produces the same unparseable response"
    );
}

#[test]
fn classification_ignores_message_wording() {
    let reworded = ProviderFailure::Status {
        provider: "test".to_owned(),
        status: 429,
        message: "slow down friend".to_owned(),
        retry_after: None,
    };

    assert!(
        reworded.is_retryable(),
        "retryability comes from the status code, never from the message text"
    );

    let misleading = ProviderFailure::Status {
        provider: "test".to_owned(),
        status: 400,
        message: "rate limit 429 service unavailable 503".to_owned(),
        retry_after: None,
    };

    assert!(
        !misleading.is_retryable(),
        "substring-matching the message would wrongly retry this permanent failure"
    );
}

#[test]
fn retry_after_is_surfaced_only_where_a_server_sent_one() {
    let with_hint = ProviderFailure::Status {
        provider: "test".to_owned(),
        status: 429,
        message: "slow down".to_owned(),
        retry_after: Some(Duration::from_secs(30)),
    };

    assert_eq!(
        with_hint.retry_after(),
        Some(Duration::from_secs(30)),
        "the server's own guidance must reach the retry policy"
    );
    assert_eq!(
        transport().retry_after(),
        None,
        "a request that never arrived carries no server guidance"
    );
}

#[test]
fn a_tool_failure_becomes_a_flagged_result_the_model_can_read() {
    let failure = ToolFailure::InvalidArguments {
        name: "read_file".to_owned(),
        reason: "`path` is required".to_owned(),
    };

    let result = failure.into_result("call-1");

    assert_eq!(
        result.tool_use_id, "call-1",
        "the result must address the call that asked for it"
    );
    assert!(result.is_error, "the model should know this was a failure");
    assert!(
        result.content.contains("`path` is required"),
        "the correction must survive into text the model reads, got: {}",
        result.content
    );
}

#[test]
fn unknown_tool_names_the_tool_that_was_asked_for() {
    let failure = ToolFailure::Unknown {
        name: "delete_everything".to_owned(),
        available: vec!["read_file".to_owned()],
    };

    let rendered = failure.to_string();

    assert!(
        rendered.contains("delete_everything"),
        "the model needs to see which name it got wrong, got: {rendered}"
    );
}
