use grizzly_agent_core::StopReason;

use super::*;

/// Feed `body` through a fresh [`ChunkedReader`], split into pieces of
/// `chunk_bytes` so line boundaries can land anywhere, and collect every
/// item it queues.
fn read_split(body: &[u8], chunk_bytes: usize) -> Vec<Item> {
    let mut reader = ChunkedReader::new("test-provider".to_owned());
    for piece in body.chunks(chunk_bytes.max(1)) {
        reader.absorb(piece);
    }
    reader.end();
    let mut items = Vec::new();
    while let Some(item) = reader.pop() {
        items.push(item);
    }
    items
}

fn read_whole(body: &[u8]) -> Vec<Item> {
    read_split(body, body.len().max(1))
}

fn expect_events(items: Vec<Item>) -> Vec<CompletionEvent> {
    items
        .into_iter()
        .map(|item| {
            item.unwrap_or_else(|err| {
                panic!("expected only successful events, got a failure: {err}")
            })
        })
        .collect()
}

fn expect_failure(items: Vec<Item>) -> ProviderFailure {
    let mut items = items;
    let last = items.pop().expect("at least one item was queued");
    assert!(
        items.iter().all(Result::is_ok),
        "a failure must be the final item, not an earlier one"
    );
    match last {
        Err(failure) => failure,
        Ok(event) => panic!("expected the stream to end in failure, got a final event: {event:?}"),
    }
}

const TEXT_STREAM: &[u8] = concat!(
    "data: {\"model\":\"gpt-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"The \"}}]}\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"answer is 4.\"}}]}\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":6}}\n",
    "data: [DONE]\n",
)
.as_bytes();

#[test]
fn a_text_stream_reassembles_in_order_ending_with_finished() {
    let events = expect_events(read_whole(TEXT_STREAM));

    assert_eq!(
        events,
        vec![
            CompletionEvent::TextDelta("The ".to_owned()),
            CompletionEvent::TextDelta("answer is 4.".to_owned()),
            CompletionEvent::Usage(grizzly_agent_core::Usage {
                input_tokens: Some(12),
                output_tokens: Some(6),
            }),
            CompletionEvent::Finished {
                stop_reason: StopReason::EndOfTurn,
                raw_stop_reason: "stop".to_owned(),
                model: "gpt-test".to_owned(),
            },
        ],
        "usage arriving after finish_reason must still land before Finished, \
         which must be the stream's last event"
    );
}

#[test]
fn the_text_stream_reassembles_identically_split_at_every_byte_boundary() {
    let whole = read_whole(TEXT_STREAM);
    let whole_events = expect_events(whole);

    for chunk_bytes in 1..=TEXT_STREAM.len() {
        let split_events = expect_events(read_split(TEXT_STREAM, chunk_bytes));
        assert_eq!(
            split_events, whole_events,
            "reassembly must not depend on where the transport splits the bytes \
             (split every {chunk_bytes} bytes)"
        );
    }
}

#[test]
fn a_multibyte_character_split_across_transport_chunks_survives() {
    // The emoji is written directly (not JSON-escaped), so its raw 4-byte
    // UTF-8 encoding sits in the wire bytes — exactly what a chunk boundary
    // can land in the middle of.
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"pong \u{1F60A}\"}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        "data: [DONE]\n",
    )
    .as_bytes();

    for chunk_bytes in 1..=body.len() {
        let events = expect_events(read_split(body, chunk_bytes));
        assert_eq!(
            events.first(),
            Some(&CompletionEvent::TextDelta("pong \u{1F60A}".to_owned())),
            "split every {chunk_bytes} bytes must still decode the emoji whole"
        );
    }
}

#[test]
fn reasoning_deltas_stay_distinct_from_text_and_round_trip_a_signature() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"checking the math\"}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"4\"}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        "data: [DONE]\n",
    )
    .as_bytes();

    let events = expect_events(read_whole(body));
    assert_eq!(
        events.first(),
        Some(&CompletionEvent::ReasoningDelta(
            "checking the math".to_owned()
        )),
        "reasoning_content must map to a reasoning delta, not the text delta"
    );
    assert!(
        events.contains(&CompletionEvent::TextDelta("4".to_owned())),
        "the visible reply is still delivered as its own text delta"
    );
}

#[test]
fn a_tool_call_stream_reassembles_fragmented_arguments_into_id_addressed_events() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[",
        "{\"index\":0,\"id\":\"call_9f3a\",\"function\":{\"name\":\"bash\",\"arguments\":\"\"}}",
        "]}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[",
        "{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\": \"}}",
        "]}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[",
        "{\"index\":0,\"function\":{\"arguments\":\"\\\"ls\\\"}\"}}",
        "]}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n",
        "data: [DONE]\n",
    )
    .as_bytes();

    let events = expect_events(read_whole(body));
    assert_eq!(
        events,
        vec![
            CompletionEvent::ToolUseStart {
                id: "call_9f3a".to_owned(),
                name: "bash".to_owned(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "call_9f3a".to_owned(),
                fragment: String::new(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "call_9f3a".to_owned(),
                fragment: "{\"command\": ".to_owned(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "call_9f3a".to_owned(),
                fragment: "\"ls\"}".to_owned(),
            },
            CompletionEvent::Finished {
                stop_reason: StopReason::ToolUse,
                raw_stop_reason: "tool_calls".to_owned(),
                model: String::new(),
            },
        ]
    );
}

#[test]
fn a_stream_that_ends_without_a_finish_reason_fails_retryably() {
    let body =
        b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"half a th\"}}]}\n".as_slice();

    let failure = expect_failure(read_whole(body));
    assert!(
        failure.is_retryable(),
        "a missing finish signal must be retryable, got {failure}"
    );
}

#[test]
fn done_with_no_finish_reason_also_fails_retryably() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n",
        "data: [DONE]\n",
    )
    .as_bytes();

    let failure = expect_failure(read_whole(body));
    assert!(failure.is_retryable());
}

#[test]
fn a_mid_stream_error_payload_is_a_non_retryable_failure_naming_the_endpoints_message() {
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"start\"}}]}\n",
        "data: {\"error\":{\"message\":\"context length exceeded\",\"type\":\"invalid_request_error\"}}\n",
    )
    .as_bytes();

    let failure = expect_failure(read_whole(body));
    assert!(
        !failure.is_retryable(),
        "a mid-stream rejection should not be retried blindly"
    );
    assert!(
        failure.to_string().contains("context length exceeded"),
        "the endpoint's own message must survive to the caller, got: {failure}"
    );
}

#[test]
fn a_malformed_chunk_is_a_decode_failure() {
    let body = b"data: {\"choices\": not json}\n".as_slice();

    let failure = expect_failure(read_whole(body));
    assert!(
        !failure.is_retryable(),
        "a decode failure must not be retried: the same bytes decode the same way every time"
    );
    assert!(matches!(failure, ProviderFailure::Decode { .. }));
}

#[test]
fn an_ignorable_line_between_events_is_skipped() {
    let body = concat!(
        ": keep-alive\n",
        "\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"}}]}\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n",
        "data: [DONE]\n",
    )
    .as_bytes();

    let events = expect_events(read_whole(body));
    assert_eq!(
        events.first(),
        Some(&CompletionEvent::TextDelta("hi".to_owned()))
    );
}
