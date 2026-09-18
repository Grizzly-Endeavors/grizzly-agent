//! When a failed model call is worth retrying, and how long to wait first.
//!
//! Split in two, as the design requires: [`decide`] is a pure function from
//! an attempt number and a failure to a wait duration or a stop, and
//! [`run_with_retry`] is the combinator that drives an operation through it,
//! applying full jitter and sleeping via injected functions so tests run on a
//! deterministic schedule with no real time.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::error::ProviderFailure;

/// An async sleep, injected so retry tests run without waiting in real time.
pub(crate) type SleepFn =
    Arc<dyn Fn(Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// A full-jitter source: given a computed delay, returns the delay to
/// actually wait, uniform between zero and that value.
pub(crate) type JitterFn = Arc<dyn Fn(Duration) -> Duration + Send + Sync>;

/// How many times to attempt a model call, and how long to wait between
/// attempts.
///
/// The default is gantry's proven schedule: 5 total attempts, exponential
/// backoff starting at 500ms and doubling, each delay capped at 10s, with
/// full jitter. Use [`RetryPolicy::none`] to disable retries entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    max_attempts: u32,
    initial_backoff: Duration,
    backoff_cap: Duration,
}

impl RetryPolicy {
    /// A custom schedule: up to `max_attempts` total attempts (at least 1),
    /// with exponential backoff starting at `initial_backoff` and doubling
    /// each attempt, capped at `backoff_cap`.
    #[must_use]
    pub fn new(max_attempts: u32, initial_backoff: Duration, backoff_cap: Duration) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            initial_backoff,
            backoff_cap,
        }
    }

    /// No retries: a single attempt only.
    ///
    /// For callers — such as evals measuring endpoint health — that want the
    /// raw outcome of one call rather than this crate's retry behavior.
    #[must_use]
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            initial_backoff: Duration::ZERO,
            backoff_cap: Duration::ZERO,
        }
    }

    /// The total number of attempts this policy allows, including the first.
    #[must_use]
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(5, Duration::from_millis(500), Duration::from_secs(10))
    }
}

/// The outcome of [`decide`]: wait this long before the next attempt, or
/// stop and hand the failure back to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RetryDecision {
    Wait(Duration),
    Stop,
}

/// Decide whether `attempt` (1-indexed, the attempt that just failed with
/// `failure`) should be retried under `policy`.
///
/// Pure: no sleeping, no randomness. The returned [`RetryDecision::Wait`]
/// duration already folds in the failure's `Retry-After` as a floor — the
/// wait is the larger of that value and the computed exponential-backoff
/// delay — before [`run_with_retry`] applies jitter on top.
pub(crate) fn decide(
    policy: &RetryPolicy,
    attempt: u32,
    failure: &ProviderFailure,
) -> RetryDecision {
    if !failure.is_retryable() || attempt >= policy.max_attempts {
        return RetryDecision::Stop;
    }

    let backoff = exponential_backoff(policy, attempt);
    let computed = match failure.retry_after() {
        Some(retry_after) => backoff.max(retry_after),
        None => backoff,
    };
    RetryDecision::Wait(computed)
}

fn exponential_backoff(policy: &RetryPolicy, attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1);
    let multiplier = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
    policy
        .initial_backoff
        .checked_mul(multiplier)
        .unwrap_or(Duration::MAX)
        .min(policy.backoff_cap)
}

/// Drive `attempt_op` to completion, retrying per `policy` on a retryable
/// failure. `sleep` and `jitter` are called once per retry, in that order:
/// `jitter` turns [`decide`]'s computed delay into the delay actually
/// waited, and `sleep` waits it.
pub(crate) async fn run_with_retry<T, Op, Fut>(
    policy: &RetryPolicy,
    sleep: &SleepFn,
    jitter: &JitterFn,
    mut attempt_op: Op,
) -> Result<T, ProviderFailure>
where
    Op: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, ProviderFailure>>,
{
    let mut attempt = 1_u32;
    loop {
        match attempt_op(attempt).await {
            Ok(value) => return Ok(value),
            Err(failure) => match decide(policy, attempt, &failure) {
                RetryDecision::Stop => return Err(failure),
                RetryDecision::Wait(computed) => {
                    let jittered = (jitter.as_ref())(computed);
                    (sleep.as_ref())(jittered).await;
                    attempt += 1;
                }
            },
        }
    }
}

/// The production sleep: a real `tokio` timer.
pub(crate) fn tokio_sleep() -> SleepFn {
    Arc::new(|duration| Box::pin(tokio::time::sleep(duration)))
}

/// The production full-jitter source: uniform in `[0, computed]`.
pub(crate) fn full_jitter() -> JitterFn {
    Arc::new(|computed| {
        if computed.is_zero() {
            return computed;
        }
        let fraction: f64 = rand::random();
        computed.mul_f64(fraction)
    })
}

#[cfg(test)]
#[path = "tests/retry.rs"]
mod tests;
