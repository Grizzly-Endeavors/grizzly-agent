use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use grizzly_agent_core::{
    Agent, Completion, Content, Message, Model, Provider, ProviderFailure, RunEnding, RunRecord,
    ScriptedProvider, ScriptedResponse, StopReason, ToolHandler, ToolSet, Usage,
};
use tempfile::TempDir;

use crate::case::CaseMeta;
use crate::check::CheckResult;
use crate::verdict::{Verdict, VerdictCategory};

use super::*;

fn text_completion(text: &str) -> Completion {
    Completion {
        content: vec![Content::Text(text.to_owned())],
        usage: Usage {
            input_tokens: Some(5),
            output_tokens: Some(2),
        },
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "scripted-model".to_owned(),
    }
}

fn model_from(provider: impl Provider + 'static) -> Model {
    Model::builder(Arc::new(provider), "scripted-model")
        .retry_policy(grizzly_agent_core::RetryPolicy::none())
        .build()
}

fn empty_tools() -> ToolSet {
    ToolSet::new(Vec::<Box<dyn ToolHandler>>::new()).expect("no tools registers cleanly")
}

/// A repeat's scratch directory: a real temp directory on disk, kept alive
/// for the repeat's lifetime and read back by `check`.
struct ScratchDir {
    dir: TempDir,
}

impl ScratchDir {
    fn marker_path(&self) -> PathBuf {
        self.dir.path().join("marker.txt")
    }
}

/// A repeat that sets up a scratch directory, runs a scripted agent that
/// replies without touching it, and passes when the run completed and the
/// marker `set_up` seeded is still there.
struct PassingCase {
    meta: CaseMeta,
}

#[async_trait]
impl AgentEvalCase for PassingCase {
    type Environment = ScratchDir;

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider =
            ScriptedProvider::new([ScriptedResponse::Completion(text_completion("done"))]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<(ScratchDir, Vec<Message>), String> {
        let dir = TempDir::new().map_err(|err| format!("failed to create scratch dir: {err}"))?;
        let environment = ScratchDir { dir };
        std::fs::write(environment.marker_path(), "seeded")
            .map_err(|err| format!("failed to seed marker: {err}"))?;
        Ok((environment, vec![Message::user("say done")]))
    }

    async fn check(&self, environment: &ScratchDir) -> Result<Vec<CheckResult>, String> {
        let content = std::fs::read_to_string(environment.marker_path())
            .map_err(|err| format!("failed to read marker: {err}"))?;
        Ok(vec![CheckResult::passed("marker-present", content)])
    }

    fn score(&self, record: &RunRecord, checks: &[CheckResult]) -> Verdict {
        if record.ending == RunEnding::Completed && checks.iter().all(|check| check.passed) {
            Verdict::pass("agent completed and the marker survived")
        } else {
            Verdict::wrong("expected a completed run with the marker present")
        }
    }
}

#[tokio::test]
async fn a_passing_repeat_carries_no_transcript() {
    let case = PassingCase {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("passing")
        },
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Pass,
        "a completed run with the marker present must verdict Pass"
    );
    assert!(
        repeat.verdict.usage.is_some(),
        "usage must be recorded on a passing repeat"
    );
    assert_eq!(
        repeat
            .detail
            .get("round_count")
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "the detail must record the round count even on a pass"
    );
    assert_eq!(
        repeat
            .detail
            .get("checks")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1),
        "the detail must record the check results even on a pass"
    );
    assert!(
        repeat.detail.get("transcript").is_none(),
        "a passing repeat must not carry the full transcript"
    );
}

/// A repeat whose agent answers, but whose scorer reads the reply and
/// decides it was wrong.
struct ScorerMissCase {
    meta: CaseMeta,
}

#[async_trait]
impl AgentEvalCase for ScorerMissCase {
    type Environment = ();

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider = ScriptedProvider::new([ScriptedResponse::Completion(text_completion(
            "the wrong answer",
        ))]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<((), Vec<Message>), String> {
        Ok(((), vec![Message::user("what is the answer?")]))
    }

    async fn check(&self, (): &()) -> Result<Vec<CheckResult>, String> {
        Ok(Vec::new())
    }

    fn score(&self, record: &RunRecord, _checks: &[CheckResult]) -> Verdict {
        if record.reply.as_deref() == Some("the answer") {
            Verdict::pass("correct")
        } else {
            Verdict::wrong(format!("got {:?}, expected \"the answer\"", record.reply))
        }
    }
}

#[tokio::test]
async fn a_scorer_miss_keeps_the_full_transcript() {
    let case = ScorerMissCase {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("scorer-miss")
        },
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Wrong,
        "a scorer that rejects the reply must verdict Wrong"
    );
    assert!(
        repeat.detail.get("transcript").is_some(),
        "a scorer miss must keep the full transcript in the detail"
    );
}

/// A repeat whose run completes fine, but whose check step cannot itself be
/// carried out — distinct from a check that ran and reported a failure.
struct CheckErrorCase {
    meta: CaseMeta,
    scored: AtomicBool,
}

#[async_trait]
impl AgentEvalCase for CheckErrorCase {
    type Environment = ();

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider =
            ScriptedProvider::new([ScriptedResponse::Completion(text_completion("done"))]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<((), Vec<Message>), String> {
        Ok(((), vec![Message::user("go")]))
    }

    async fn check(&self, (): &()) -> Result<Vec<CheckResult>, String> {
        Err("hidden command exited nonzero before it could report".to_owned())
    }

    fn score(&self, _record: &RunRecord, _checks: &[CheckResult]) -> Verdict {
        self.scored.store(true, Ordering::SeqCst);
        Verdict::pass("must never be reached — check failed first")
    }
}

#[tokio::test]
async fn a_check_error_fails_the_repeat_without_scoring() {
    let case = CheckErrorCase {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("check-error")
        },
        scored: AtomicBool::new(false),
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Failed,
        "an error carrying out the checks must verdict Failed"
    );
    assert!(
        repeat.verdict.reason.contains("hidden command"),
        "the failed verdict must carry the check error as its reason: {}",
        repeat.verdict.reason
    );
    assert!(
        !case.scored.load(Ordering::SeqCst),
        "the scorer must not run when check itself errors"
    );
    assert!(
        repeat.detail.get("transcript").is_some(),
        "a check-error repeat must still keep the transcript from the run that did complete"
    );
}

/// A repeat whose environment can never be set up.
struct SetUpErrorCase {
    meta: CaseMeta,
}

#[async_trait]
impl AgentEvalCase for SetUpErrorCase {
    type Environment = ();

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider =
            ScriptedProvider::new([ScriptedResponse::Completion(text_completion("unreachable"))]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<((), Vec<Message>), String> {
        Err("scratch directory quota exceeded".to_owned())
    }

    async fn check(&self, (): &()) -> Result<Vec<CheckResult>, String> {
        panic!("check must not run when set up failed");
    }

    fn score(&self, _record: &RunRecord, _checks: &[CheckResult]) -> Verdict {
        panic!("score must not run when set up failed");
    }
}

#[tokio::test]
async fn a_set_up_error_fails_the_repeat_before_the_agent_runs() {
    let case = SetUpErrorCase {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("set-up-error")
        },
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Failed,
        "an error setting up the environment must verdict Failed"
    );
    assert!(
        repeat.verdict.reason.contains("quota exceeded"),
        "the failed verdict must carry the set-up error as its reason: {}",
        repeat.verdict.reason
    );
    assert!(
        repeat.detail.is_null(),
        "there is no run to report on when set up never produced one"
    );
}

/// A repeat whose provider fails outright.
struct ProviderFailureCase {
    meta: CaseMeta,
    checked: AtomicBool,
}

#[async_trait]
impl AgentEvalCase for ProviderFailureCase {
    type Environment = ();

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider = ScriptedProvider::new([ScriptedResponse::PreStreamFailure(
            ProviderFailure::Transport {
                provider: "test".to_owned(),
                source: Box::new(std::io::Error::other("connection refused")),
            },
        )]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<((), Vec<Message>), String> {
        Ok(((), vec![Message::user("go")]))
    }

    async fn check(&self, (): &()) -> Result<Vec<CheckResult>, String> {
        self.checked.store(true, Ordering::SeqCst);
        Ok(Vec::new())
    }

    fn score(&self, _record: &RunRecord, _checks: &[CheckResult]) -> Verdict {
        Verdict::pass("must never be reached — the provider failed first")
    }
}

#[tokio::test]
async fn a_provider_failure_is_unavailable_and_skips_check_and_score() {
    let case = ProviderFailureCase {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("provider-failure")
        },
        checked: AtomicBool::new(false),
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Unavailable,
        "a provider failure must verdict Unavailable"
    );
    assert!(
        !case.checked.load(Ordering::SeqCst),
        "check must not run after a provider failure"
    );
    assert!(
        repeat.detail.get("ending").is_none(),
        "a provider failure never reached a RunEnding, so the detail must not claim one"
    );
    assert!(
        repeat.detail.get("transcript").is_some(),
        "the failure's trace must be kept in the detail even with zero completed rounds"
    );
}

/// A repeat whose task input itself breaks the loop's invariants.
struct InvalidConversationCase {
    meta: CaseMeta,
}

#[async_trait]
impl AgentEvalCase for InvalidConversationCase {
    type Environment = ();

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider =
            ScriptedProvider::new([ScriptedResponse::Completion(text_completion("unreachable"))]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<((), Vec<Message>), String> {
        // An Agent's sections are its only system prompt; a system-role
        // message in the conversation breaks that invariant.
        Ok(((), vec![Message::system("nope")]))
    }

    async fn check(&self, (): &()) -> Result<Vec<CheckResult>, String> {
        Ok(Vec::new())
    }

    fn score(&self, _record: &RunRecord, _checks: &[CheckResult]) -> Verdict {
        Verdict::pass("unreachable")
    }
}

#[tokio::test]
async fn an_invalid_conversation_fails_the_repeat() {
    let case = InvalidConversationCase {
        meta: CaseMeta {
            repeats: Some(1),
            ..CaseMeta::new("invalid-conversation")
        },
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let repeat = reports
        .first()
        .and_then(|report| report.repeats.first())
        .expect("one repeat must have run");

    assert_eq!(
        repeat.verdict.category,
        VerdictCategory::Failed,
        "an invalid conversation is a case bug, not an endpoint outage, so it must verdict Failed"
    );
}

/// A repeat that tracks how many repeats of it are in flight at once.
struct ConcurrencyProbeCase {
    meta: CaseMeta,
    delay: Duration,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: Arc<AtomicUsize>,
}

#[async_trait]
impl AgentEvalCase for ConcurrencyProbeCase {
    type Environment = ();

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_agent(&self) -> Agent {
        let provider =
            ScriptedProvider::new([ScriptedResponse::Completion(text_completion("done"))]);
        Agent::builder(model_from(provider), empty_tools()).build()
    }

    async fn set_up(&self) -> Result<((), Vec<Message>), String> {
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(current, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(((), vec![Message::user("go")]))
    }

    async fn check(&self, (): &()) -> Result<Vec<CheckResult>, String> {
        Ok(Vec::new())
    }

    fn score(&self, _record: &RunRecord, _checks: &[CheckResult]) -> Verdict {
        Verdict::pass("ok")
    }
}

#[tokio::test]
async fn repeats_run_sequentially_by_default() {
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let case = ConcurrencyProbeCase {
        meta: CaseMeta {
            repeats: Some(4),
            ..CaseMeta::new("sequential")
        },
        delay: Duration::from_millis(20),
        in_flight: Arc::new(AtomicUsize::new(0)),
        max_in_flight: Arc::clone(&max_in_flight),
    };
    let runner = AgentEvalRunner::builder().build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let report = reports.first().expect("one case must produce one report");

    assert_eq!(report.repeats.len(), 4, "all four repeats must have run");
    assert_eq!(
        max_in_flight.load(Ordering::SeqCst),
        1,
        "the default concurrency must run repeats one at a time"
    );
}

#[tokio::test]
async fn repeats_are_bounded_by_a_raised_concurrency_limit() {
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let case = ConcurrencyProbeCase {
        meta: CaseMeta {
            repeats: Some(6),
            ..CaseMeta::new("bounded")
        },
        delay: Duration::from_millis(30),
        in_flight: Arc::new(AtomicUsize::new(0)),
        max_in_flight: Arc::clone(&max_in_flight),
    };
    let runner = AgentEvalRunner::builder().concurrency(2).build();

    let reports = runner.run(std::slice::from_ref(&case)).await;
    let report = reports.first().expect("one case must produce one report");

    assert_eq!(report.repeats.len(), 6, "all six repeats must have run");
    assert!(
        max_in_flight.load(Ordering::SeqCst) <= 2,
        "concurrency must never exceed the configured limit of 2, saw {}",
        max_in_flight.load(Ordering::SeqCst)
    );
    assert!(
        max_in_flight.load(Ordering::SeqCst) >= 2,
        "with 6 repeats and a limit of 2, at least two repeats must overlap"
    );
}
