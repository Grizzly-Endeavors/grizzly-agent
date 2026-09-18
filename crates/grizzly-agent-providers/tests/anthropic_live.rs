//! Live round trip against the real Anthropic Messages API. Ignored by
//! default; run explicitly with the environment variables below set:
//!
//! ```sh
//! ANTHROPIC_API_KEY=sk-ant-... \
//! GRIZZLY_AGENT_TEST_ANTHROPIC_MODEL=claude-haiku-4-5 \
//! cargo test -p grizzly-agent-providers --features anthropic --test anthropic_live -- --ignored
//! ```
#![cfg(feature = "anthropic")]
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use std::sync::Arc;

use grizzly_agent_core::{CompletionRequest, Content, Message, Model, Role, ToolResult, ToolSpec};
use grizzly_agent_providers::AnthropicProvider;

/// Reads a required env var for this live test, or panics naming it —
/// legible when run explicitly, since a missing var otherwise fails deep
/// inside provider construction with no clue which one was left unset.
macro_rules! required_env {
    ($name:expr) => {
        std::env::var($name).unwrap_or_else(|_| panic!("set {} to run this live test", $name))
    };
}

#[tokio::test]
#[ignore = "hits the real Anthropic API; opt in with the env vars documented above"]
async fn a_simple_prompt_and_a_one_tool_round_trip_complete() {
    let api_key = required_env!("ANTHROPIC_API_KEY");
    let model_id = required_env!("GRIZZLY_AGENT_TEST_ANTHROPIC_MODEL");

    let provider = AnthropicProvider::builder(api_key, &model_id)
        .build()
        .expect("the provider always builds with a well-formed key and default base url");
    let model = Model::builder(Arc::new(provider), &model_id)
        .default_max_tokens(256)
        .build();

    let completion = model
        .complete(CompletionRequest::new(vec![Message::user(
            "Reply with exactly one word: hello",
        )]))
        .await
        .expect("a simple prompt must complete");
    assert!(
        !completion.content.is_empty(),
        "the model must reply with something"
    );

    let weather_tool = ToolSpec {
        name: "get_weather".into(),
        description: "Get the current weather for a city".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
        }),
    };
    let mut request = CompletionRequest::new(vec![Message::user(
        "What is the weather in Boston? Use the get_weather tool.",
    )]);
    request.tools = vec![weather_tool];
    let first = model
        .complete(request.clone())
        .await
        .expect("a tool-advertising prompt must complete");
    let tool_use = first
        .content
        .iter()
        .find_map(|block| match block {
            Content::ToolUse(tool_use) => Some(tool_use),
            Content::Text(_) | Content::Reasoning { .. } | Content::ToolResult(_) => None,
        })
        .unwrap_or_else(|| panic!("expected a tool call, got {:?}", first.content));

    let mut messages = request.messages;
    messages.push(Message {
        role: Role::Assistant,
        content: first.content.clone(),
    });
    messages.push(Message::tool_results(vec![ToolResult {
        tool_use_id: tool_use.id.clone(),
        content: "72F and sunny".to_owned(),
        is_error: false,
    }]));
    let second = model
        .complete(CompletionRequest::new(messages))
        .await
        .expect("the follow-up with the tool result must complete");
    assert!(
        !second.content.is_empty(),
        "the model must answer after seeing the tool result"
    );
}
