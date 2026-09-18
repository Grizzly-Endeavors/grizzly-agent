use grizzly_agent_core::{Completion, CompletionRequest, Message};

use crate::case::CaseMeta;
use crate::verdict::Verdict;

use super::*;

struct MinimalCase {
    meta: CaseMeta,
}

impl ResponseEvalCase for MinimalCase {
    type Answer = String;

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_request(&self) -> CompletionRequest {
        CompletionRequest::new(vec![Message::user("hi")])
    }

    fn parse(&self, completion: &Completion) -> Result<Self::Answer, String> {
        Ok(completion.model.clone())
    }

    fn score(&self, _answer: &Self::Answer) -> Verdict {
        Verdict::pass("ok")
    }
}

#[test]
fn a_case_that_overrides_nothing_defers_its_timeout_to_the_runner() {
    let case = MinimalCase {
        meta: CaseMeta::new("minimal"),
    };
    assert_eq!(
        case.timeout(),
        CaseTimeout::Default,
        "a case with no timeout override must report CaseTimeout::Default"
    );
}

struct CustomTimeoutCase {
    meta: CaseMeta,
}

impl ResponseEvalCase for CustomTimeoutCase {
    type Answer = String;

    fn meta(&self) -> &CaseMeta {
        &self.meta
    }

    fn build_request(&self) -> CompletionRequest {
        CompletionRequest::new(vec![Message::user("hi")])
    }

    fn parse(&self, completion: &Completion) -> Result<Self::Answer, String> {
        Ok(completion.model.clone())
    }

    fn score(&self, _answer: &Self::Answer) -> Verdict {
        Verdict::pass("ok")
    }

    fn timeout(&self) -> CaseTimeout {
        CaseTimeout::Custom(std::time::Duration::from_secs(5))
    }
}

#[test]
fn a_case_may_set_its_own_timeout() {
    let case = CustomTimeoutCase {
        meta: CaseMeta::new("custom"),
    };
    assert_eq!(
        case.timeout(),
        CaseTimeout::Custom(std::time::Duration::from_secs(5)),
        "a case overriding timeout must report its own duration"
    );
}
