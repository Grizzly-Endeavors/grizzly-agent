use super::*;

#[test]
fn a_fresh_context_has_no_stop_request() {
    let mut context = ToolContext::new(CancellationToken::new());
    assert_eq!(context.take_stop_request(), None);
}

#[test]
fn request_stop_records_reply_and_reason() {
    let mut context = ToolContext::new(CancellationToken::new());

    let recorded = context.request_stop("handing off to a human", "needs manual review");

    assert!(recorded, "the only caller so far must win");
    assert_eq!(
        context.take_stop_request(),
        Some(StopRequest {
            reply: "handing off to a human".to_owned(),
            reason: "needs manual review".to_owned(),
        })
    );
}

#[test]
fn two_stop_requests_in_one_context_keep_the_first() {
    let mut context = ToolContext::new(CancellationToken::new());

    let first_won = context.request_stop("first reply", "first reason");
    let second_won = context.request_stop("second reply", "second reason");

    assert!(first_won, "the first call must record its request");
    assert!(!second_won, "a later call must not overwrite the first");
    assert_eq!(
        context.take_stop_request(),
        Some(StopRequest {
            reply: "first reply".to_owned(),
            reason: "first reason".to_owned(),
        })
    );
}

#[test]
fn taking_the_stop_request_empties_the_slot() {
    let mut context = ToolContext::new(CancellationToken::new());
    assert!(context.request_stop("reply", "reason"));

    assert!(context.take_stop_request().is_some());
    assert_eq!(
        context.take_stop_request(),
        None,
        "a second take must find nothing left"
    );
}

#[test]
fn cancellation_token_reflects_cancellation() {
    let token = CancellationToken::new();
    let context = ToolContext::new(token.clone());

    assert!(!context.cancellation_token().is_cancelled());
    token.cancel();
    assert!(context.cancellation_token().is_cancelled());
}
