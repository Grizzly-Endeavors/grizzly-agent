use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

#[test]
fn retry_after_parses_a_plain_second_count() {
    let mut headers = HeaderMap::new();
    headers.insert(RETRY_AFTER, HeaderValue::from_static("30"));

    assert_eq!(
        retry_after_from_headers(&headers),
        Some(Duration::from_secs(30))
    );
}

#[test]
fn retry_after_is_absent_when_the_header_is_missing() {
    assert_eq!(retry_after_from_headers(&HeaderMap::new()), None);
}

#[test]
fn retry_after_is_absent_when_the_header_is_not_a_second_count() {
    let mut headers = HeaderMap::new();
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"),
    );

    assert_eq!(
        retry_after_from_headers(&headers),
        None,
        "the HTTP-date form is not parsed; no provider this crate speaks sends it"
    );
}

#[tokio::test]
async fn status_failure_carries_the_status_body_and_retry_after() {
    let server = MockServer::start().await;
    Mock::given(path("/fails"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_string("rate limited, slow down")
                .insert_header("retry-after", "5"),
        )
        .mount(&server)
        .await;

    let client = build_client(Duration::from_secs(5), HeaderMap::new()).expect("client builds");
    let response = client
        .get(format!("{}/fails", server.uri()))
        .send()
        .await
        .expect("the mock server answers");

    let failure = status_failure("test-provider", response).await;
    let grizzly_agent_core::ProviderFailure::Status {
        provider,
        status,
        message,
        retry_after,
    } = failure
    else {
        panic!("expected a Status failure");
    };
    assert_eq!(provider, "test-provider");
    assert_eq!(status, 429);
    assert_eq!(message, "rate limited, slow down");
    assert_eq!(retry_after, Some(Duration::from_secs(5)));
}

#[tokio::test]
async fn an_idle_response_past_the_timeout_is_a_retryable_transport_failure() {
    let server = MockServer::start().await;
    Mock::given(path("/idle"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("data: first\n\n")
                .set_delay(Duration::from_millis(200)),
        )
        .mount(&server)
        .await;

    // `read_timeout` bounds every read on the connection, including the wait
    // for the response itself — so a server that sits silent past it fails
    // here exactly as it would mid-stream, on a real idle connection.
    let client = build_client(Duration::from_millis(20), HeaderMap::new()).expect("client builds");
    let outcome = client.get(format!("{}/idle", server.uri())).send().await;

    let failure = transport_failure(
        "test-provider",
        outcome.expect_err("the connection must time out before the delayed response arrives"),
    );
    assert!(
        failure.is_retryable(),
        "an idle-timeout transport failure must be retryable"
    );
}
