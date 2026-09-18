//! Live round trip against a real OpenAI-compatible endpoint. Ignored by
//! default; run explicitly with the environment variables below set:
//!
//! ```sh
//! GRIZZLY_AGENT_TEST_OPENAI_ENDPOINT=http://localhost:11434/v1 \
//! GRIZZLY_AGENT_TEST_OPENAI_MODEL=llama3.1 \
//! cargo test -p grizzly-agent-providers --features openai --test openai_live -- --ignored
//! ```
//!
//! `GRIZZLY_AGENT_TEST_OPENAI_API_KEY` is optional, for endpoints that
//! require one (Ollama Cloud, a hosted gateway); a local Ollama needs none.
#![cfg(feature = "openai")]
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use std::sync::Arc;

use grizzly_agent_core::{CompletionRequest, Content, Message, Model, Role, ToolResult, ToolSpec};
use grizzly_agent_providers::OpenAiCompatibleProvider;

/// Reads a required env var for this live test, or panics naming it —
/// legible when run explicitly, since a missing var otherwise fails deep
/// inside provider construction with no clue which one was left unset.
macro_rules! required_env {
    ($name:expr) => {
        std::env::var($name).unwrap_or_else(|_| panic!("set {} to run this live test", $name))
    };
}

#[tokio::test]
#[ignore = "hits a real OpenAI-compatible endpoint; opt in with the env vars documented above"]
async fn a_simple_prompt_and_a_one_tool_round_trip_complete() {
    let endpoint = required_env!("GRIZZLY_AGENT_TEST_OPENAI_ENDPOINT");
    let model_id = required_env!("GRIZZLY_AGENT_TEST_OPENAI_MODEL");
    let api_key = std::env::var("GRIZZLY_AGENT_TEST_OPENAI_API_KEY").ok();

    let mut builder = OpenAiCompatibleProvider::builder(&endpoint);
    if let Some(key) = api_key {
        builder = builder.api_key(key);
    }
    let provider = builder.build().expect("the endpoint is a valid base url");
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
