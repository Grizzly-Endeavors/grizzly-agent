use grizzly_agent_core::StopReason;

use super::*;

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
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10}}}\n",
    "\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n",
    "\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"The \"}}\n",
    "\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"answer is 4.\"}}\n",
    "\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n",
    "\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":6}}\n",
    "\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n",
    "\n",
)
.as_bytes();

#[test]
fn a_text_stream_reassembles_in_order_ending_with_finished() {
    let events = expect_events(read_whole(TEXT_STREAM));

    assert_eq!(
        events,
        vec![
            CompletionEvent::Usage(grizzly_agent_core::Usage {
                input_tokens: Some(10),
                output_tokens: None,
            }),
            CompletionEvent::TextDelta("The ".to_owned()),
            CompletionEvent::TextDelta("answer is 4.".to_owned()),
            CompletionEvent::Usage(grizzly_agent_core::Usage {
                input_tokens: None,
                output_tokens: Some(6),
            }),
            CompletionEvent::Finished {
                stop_reason: StopReason::EndOfTurn,
                raw_stop_reason: "end_turn".to_owned(),
                model: "claude-test".to_owned(),
            },
        ],
        "message_delta's usage and stop reason arrive before message_stop, \
         but Finished must still be the stream's last event"
    );
}

#[test]
fn the_text_stream_reassembles_identically_split_at_every_byte_boundary() {
    let whole_events = expect_events(read_whole(TEXT_STREAM));

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
    let body = concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"pong \u{1F60A}\"}}\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n",
        "data: {\"type\":\"message_stop\"}\n",
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
fn a_tool_use_stream_reassembles_fragmented_arguments_by_block_index() {
    let body = concat!(
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":",
        "{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"bash\",\"input\":{}}}\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\"}}\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"ls\\\"}\"}}\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":4}}\n",
        "data: {\"type\":\"message_stop\"}\n",
    )
    .as_bytes();

    let events = expect_events(read_whole(body));
    assert_eq!(
        events,
        vec![
            CompletionEvent::ToolUseStart {
                id: "toolu_1".to_owned(),
                name: "bash".to_owned(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "toolu_1".to_owned(),
                fragment: "{\"cmd\":".to_owned(),
            },
            CompletionEvent::ToolUseArgumentsDelta {
                id: "toolu_1".to_owned(),
                fragment: "\"ls\"}".to_owned(),
            },
            CompletionEvent::Usage(grizzly_agent_core::Usage {
                input_tokens: None,
                output_tokens: Some(4),
            }),
            CompletionEvent::Finished {
                stop_reason: StopReason::ToolUse,
                raw_stop_reason: "tool_use".to_owned(),
                model: String::new(),
            },
        ]
    );
}

#[test]
fn a_reasoning_round_trip_carries_the_signature() {
    let body = concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"thinking_delta\",\"thinking\":\"checking\"}}\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"signature_delta\",\"signature\":\"sig-xyz\"}}\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n",
        "data: {\"type\":\"message_stop\"}\n",
    )
    .as_bytes();

    let events = expect_events(read_whole(body));
    assert_eq!(
        events.first(),
        Some(&CompletionEvent::ReasoningDelta("checking".to_owned()))
    );
    assert!(events.contains(&CompletionEvent::ReasoningSignatureDelta(
        "sig-xyz".to_owned()
    )));
}

#[test]
fn a_stream_that_ends_without_message_stop_fails_retryably() {
    let body = concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"text_delta\",\"text\":\"half a th\"}}\n",
    )
    .as_bytes();

    let failure = expect_failure(read_whole(body));
    assert!(
        failure.is_retryable(),
        "a missing finish signal must be retryable"
    );
}

#[test]
fn message_stop_with_no_prior_message_delta_also_fails_retryably() {
    let body = b"data: {\"type\":\"message_stop\"}\n".as_slice();

    let failure = expect_failure(read_whole(body));
    assert!(failure.is_retryable());
}

#[test]
fn a_mid_stream_error_event_maps_the_documented_type_to_a_retryable_status() {
    let body = concat!(
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"text_delta\",\"text\":\"start\"}}\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n",
    )
    .as_bytes();

    let failure = expect_failure(read_whole(body));
    assert!(
        failure.is_retryable(),
        "overloaded_error maps to a 5xx-class status and must be retryable"
    );
    assert!(failure.to_string().contains("Overloaded"));
}

#[test]
fn an_invalid_request_error_event_is_not_retryable() {
    let body = concat!(
        "data: {\"type\":\"error\",\"error\":",
        "{\"type\":\"invalid_request_error\",\"message\":\"context too long\"}}\n",
    )
    .as_bytes();

    let failure = expect_failure(read_whole(body));
    assert!(!failure.is_retryable());
}

#[test]
fn a_malformed_chunk_is_a_decode_failure() {
    let body = b"data: {\"type\": not json}\n".as_slice();

    let failure = expect_failure(read_whole(body));
    assert!(!failure.is_retryable());
    assert!(matches!(failure, ProviderFailure::Decode { .. }));
}

#[test]
fn ping_lines_are_skipped() {
    let body = concat!(
        "data: {\"type\":\"ping\"}\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"text_delta\",\"text\":\"hi\"}}\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n",
        "data: {\"type\":\"message_stop\"}\n",
    )
    .as_bytes();

    let events = expect_events(read_whole(body));
    assert_eq!(
        events.first(),
        Some(&CompletionEvent::TextDelta("hi".to_owned()))
    );
}
