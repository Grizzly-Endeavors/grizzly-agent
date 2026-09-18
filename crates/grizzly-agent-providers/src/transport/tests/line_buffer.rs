use super::*;

/// Feed `body` through a fresh [`LineBuffer`] `chunk_bytes` at a time, plus
/// whatever the trailing [`LineBuffer::flush`] returns.
fn read_split(body: &[u8], chunk_bytes: usize) -> Vec<String> {
    let mut buffer = LineBuffer::default();
    let mut lines = Vec::new();
    for piece in body.chunks(chunk_bytes.max(1)) {
        lines.extend(buffer.push(piece));
    }
    if let Some(trailing) = buffer.flush() {
        lines.push(trailing);
    }
    lines
}

#[test]
fn a_single_push_yields_every_complete_line() {
    let mut buffer = LineBuffer::default();
    let lines = buffer.push(b"data: one\ndata: two\n");
    assert_eq!(lines, vec!["data: one", "data: two"]);
}

#[test]
fn a_line_split_across_many_pushes_arrives_whole() {
    let mut buffer = LineBuffer::default();
    assert!(buffer.push(b"da").is_empty(), "no newline yet");
    assert!(buffer.push(b"ta: par").is_empty(), "still no newline");
    let lines = buffer.push(b"tial\n");
    assert_eq!(lines, vec!["data: partial"]);
}

#[test]
fn crlf_terminators_are_stripped_like_lf() {
    let mut buffer = LineBuffer::default();
    let lines = buffer.push(b"data: one\r\ndata: two\r\n");
    assert_eq!(lines, vec!["data: one", "data: two"]);
}

#[test]
fn an_unterminated_tail_is_returned_only_on_flush() {
    let mut buffer = LineBuffer::default();
    assert!(
        buffer.push(b"data: no newline yet").is_empty(),
        "an unterminated line is not yielded by push"
    );
    assert_eq!(buffer.flush().as_deref(), Some("data: no newline yet"));
    assert_eq!(buffer.flush(), None, "flush is empty once drained");
}

#[test]
fn reassembly_is_identical_no_matter_where_chunks_are_split() {
    let body = b"data: {\"a\":1}\ndata: {\"b\":2}\ndata: [DONE]\n".as_slice();
    let whole = read_split(body, body.len());

    for chunk_bytes in 1..=body.len() {
        let split = read_split(body, chunk_bytes);
        assert_eq!(
            split, whole,
            "line reassembly must not depend on where the transport splits the bytes \
             (split every {chunk_bytes} bytes)"
        );
    }
}

#[test]
fn a_multibyte_character_split_mid_sequence_survives() {
    let body = "data: pong \u{1F60A} done\n".as_bytes();

    for split_at in 1..body.len() {
        let mut buffer = LineBuffer::default();
        let (first, second) = body.split_at(split_at);
        let mut lines = buffer.push(first);
        lines.extend(buffer.push(second));
        assert_eq!(
            lines,
            vec!["data: pong \u{1F60A} done"],
            "a 4-byte UTF-8 character split at byte {split_at} must still decode whole, \
             since the buffer never decodes before a full line is assembled"
        );
    }
}

#[test]
fn invalid_utf8_decodes_lossily_instead_of_failing() {
    let mut buffer = LineBuffer::default();
    let mut body = b"data: broken ".to_vec();
    body.push(0xFF);
    body.extend_from_slice(b"\n");

    let lines = buffer.push(&body);
    assert_eq!(lines.len(), 1, "one line, despite the invalid byte");
    let line = lines.first().expect("checked the length above");
    assert!(
        line.contains('\u{FFFD}'),
        "an invalid byte becomes the replacement character, got: {line:?}"
    );
}
