//! Tests for [`super`].

use std::time::Duration;

use super::{RoundRecord, RunEnding, RunRecord, RunTrace, ToolCallRecord};
use crate::completion::{StopReason, Usage};
use crate::tools::StopRequest;

#[test]
fn a_run_record_carries_its_ending_reply_and_trace_together() {
    let round = RoundRecord {
        usage: Usage {
            input_tokens: Some(10),
            output_tokens: Some(4),
        },
        stop_reason: StopReason::EndOfTurn,
        latency: Duration::from_millis(50),
        tool_calls: vec![ToolCallRecord {
            name: "read_file".to_owned(),
            arguments: serde_json::json!({"path": "README.md"}),
            failed: false,
            latency: Duration::from_millis(5),
        }],
    };
    let trace = RunTrace {
        messages: Vec::new(),
        rounds: vec![round.clone()],
        total_usage: round.usage,
    };
    let record = RunRecord {
        ending: RunEnding::Completed,
        reply: Some("done".to_owned()),
        trace: trace.clone(),
    };

    assert_eq!(record.ending, RunEnding::Completed);
    assert_eq!(record.reply, Some("done".to_owned()));
    assert_eq!(record.trace, trace);
}

#[test]
fn stop_requested_and_stalled_endings_carry_their_own_detail() {
    let stop_requested = RunEnding::StopRequested(StopRequest {
        reply: "handing off".to_owned(),
        reason: "needs a human".to_owned(),
    });
    let stalled = RunEnding::Stalled {
        tool: "read_file".to_owned(),
        count: 4,
    };

    assert!(matches!(
        stop_requested,
        RunEnding::StopRequested(StopRequest { reply, .. }) if reply == "handing off"
    ));
    assert!(matches!(
        stalled,
        RunEnding::Stalled { tool, count: 4 } if tool == "read_file"
    ));
}
