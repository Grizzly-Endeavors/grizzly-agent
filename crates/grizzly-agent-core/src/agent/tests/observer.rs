//! Tests for [`super`].

use super::RunObserver;
use crate::agent::trace::{RoundRecord, ToolCallRecord};
use crate::completion::{CompletionEvent, StopReason, Usage};

/// An observer that implements nothing but the trait's defaults.
struct SilentObserver;

impl RunObserver for SilentObserver {}

#[tokio::test]
async fn default_methods_are_no_ops_a_consumer_can_skip() {
    let observer = SilentObserver;

    observer
        .on_event(&CompletionEvent::TextDelta("hi".to_owned()))
        .await;
    observer
        .on_round(&RoundRecord {
            usage: Usage::default(),
            stop_reason: StopReason::EndOfTurn,
            latency: std::time::Duration::ZERO,
            tool_calls: Vec::new(),
        })
        .await;
    observer
        .on_tool_call(&ToolCallRecord {
            name: "noop".to_owned(),
            arguments: serde_json::Value::Null,
            failed: false,
            latency: std::time::Duration::ZERO,
        })
        .await;
}
