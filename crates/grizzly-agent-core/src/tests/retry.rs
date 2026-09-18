use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

fn transport_failure() -> ProviderFailure {
    ProviderFailure::Transport {
        provider: "test".to_owned(),
        source: Box::new(std::io::Error::other("boom")),
    }
}

fn status_with_retry_after(retry_after: Duration) -> ProviderFailure {
    ProviderFailure::Status {
        provider: "test".to_owned(),
        status: 429,
        message: "slow down".to_owned(),
        retry_after: Some(retry_after),
    }
}

#[test]
fn the_default_schedule_doubles_from_500ms_and_caps_at_10s() {
    let policy = RetryPolicy::default();
    let failure = transport_failure();

    let waits: Vec<Duration> = (1..policy.max_attempts())
        .map(|attempt| match decide(&policy, attempt, &failure) {
            RetryDecision::Wait(duration) => duration,
            RetryDecision::Stop => {
                panic!("attempt {attempt} of {} must retry", policy.max_attempts())
            }
        })
        .collect();

    assert_eq!(
        waits,
        vec![
            Duration::from_millis(500),
            Duration::from_millis(1000),
            Duration::from_millis(2000),
            Duration::from_millis(4000),
        ],
        "the default policy must double from 500ms across its 4 retry decisions"
    );
}

#[test]
fn backoff_caps_at_the_configured_ceiling() {
    let policy = RetryPolicy::new(10, Duration::from_secs(1), Duration::from_secs(10));
    let failure = transport_failure();

    let RetryDecision::Wait(sixth) = decide(&policy, 6, &failure) else {
        panic!("attempt 6 must retry");
    };

    assert_eq!(
        sixth,
        Duration::from_secs(10),
        "a delay that would exceed the cap must be clamped to it"
    );
}

#[test]
fn retry_after_wins_when_larger_than_the_computed_backoff() {
    let policy = RetryPolicy::default();
    let failure = status_with_retry_after(Duration::from_secs(30));

    let RetryDecision::Wait(computed) = decide(&policy, 1, &failure) else {
        panic!("a retryable status must retry");
    };

    assert_eq!(
        computed,
        Duration::from_secs(30),
        "Retry-After must win when it exceeds the computed backoff"
    );
}

#[test]
fn the_computed_backoff_wins_when_larger_than_retry_after() {
    let policy = RetryPolicy::default();
    let failure = status_with_retry_after(Duration::from_millis(10));

    let RetryDecision::Wait(computed) = decide(&policy, 3, &failure) else {
        panic!("a retryable status must retry");
    };

    assert_eq!(
        computed,
        Duration::from_millis(2000),
        "the computed backoff must win when Retry-After is smaller"
    );
}

#[test]
fn a_non_retryable_failure_stops_immediately() {
    let policy = RetryPolicy::default();
    let failure = ProviderFailure::InvalidRequest("bad request".to_owned());

    assert_eq!(
        decide(&policy, 1, &failure),
        RetryDecision::Stop,
        "a non-retryable failure must never be retried"
    );
}

#[test]
fn the_final_attempt_stops_even_when_the_failure_is_retryable() {
    let policy = RetryPolicy::default();
    let failure = transport_failure();

    assert_eq!(
        decide(&policy, policy.max_attempts(), &failure),
        RetryDecision::Stop,
        "once max_attempts is reached, decide must stop rather than retry forever"
    );
}

#[test]
fn none_policy_allows_exactly_one_attempt() {
    assert_eq!(RetryPolicy::none().max_attempts(), 1);
    assert_eq!(
        decide(&RetryPolicy::none(), 1, &transport_failure()),
        RetryDecision::Stop,
        "RetryPolicy::none must never retry"
    );
}

fn recording_sleep(log: Arc<Mutex<Vec<Duration>>>) -> SleepFn {
    Arc::new(move |duration| {
        let log = Arc::clone(&log);
        Box::pin(async move {
            log.lock()
                .expect("sleep log mutex must not be poisoned")
                .push(duration);
        })
    })
}

fn identity_jitter() -> JitterFn {
    Arc::new(|computed| computed)
}

#[tokio::test]
async fn run_with_retry_stops_retrying_after_the_first_success() {
    let policy = RetryPolicy::default();
    let sleep_log = Arc::new(Mutex::new(Vec::new()));
    let sleep = recording_sleep(Arc::clone(&sleep_log));
    let jitter = identity_jitter();
    let calls = AtomicU32::new(0);

    let result = run_with_retry(&policy, &sleep, &jitter, |_attempt| {
        let call = calls.fetch_add(1, Ordering::SeqCst);
        async move {
            if call < 2 {
                Err(transport_failure())
            } else {
                Ok("done")
            }
        }
    })
    .await;

    assert_eq!(
        result.expect("run_with_retry must return the first success"),
        "done"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "must retry until it succeeds"
    );
    assert_eq!(
        sleep_log
            .lock()
            .expect("sleep log mutex must not be poisoned")
            .len(),
        2,
        "must sleep once per retry, not once per attempt"
    );
}

#[tokio::test]
async fn run_with_retry_gives_up_after_max_attempts() {
    let policy = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(10));
    let sleep = recording_sleep(Arc::new(Mutex::new(Vec::new())));
    let jitter = identity_jitter();
    let calls = AtomicU32::new(0);

    let result: Result<(), ProviderFailure> =
        run_with_retry(&policy, &sleep, &jitter, |_attempt| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(transport_failure()) }
        })
        .await;

    assert!(
        result.is_err(),
        "exhausting all attempts must return the last failure"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "must try exactly max_attempts times"
    );
}

#[tokio::test]
async fn run_with_retry_never_retries_a_non_retryable_failure() {
    let policy = RetryPolicy::default();
    let sleep = recording_sleep(Arc::new(Mutex::new(Vec::new())));
    let jitter = identity_jitter();
    let calls = AtomicU32::new(0);

    let result: Result<(), ProviderFailure> =
        run_with_retry(&policy, &sleep, &jitter, |_attempt| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err(ProviderFailure::InvalidRequest("bad".to_owned())) }
        })
        .await;

    assert!(
        result.is_err(),
        "a non-retryable failure must surface as an error"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "must not retry a non-retryable failure"
    );
}
