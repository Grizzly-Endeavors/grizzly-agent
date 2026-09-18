//! End-to-end: a scripted [`Agent`] run activates a skill through
//! [`activate_skill_tool`], the following round's system prompt carries the
//! skill's body, and deactivating it removes the body again.
#![expect(
    clippy::tests_outside_test_module,
    reason = "integration tests live at crate root by cargo convention"
)]

use std::sync::Arc;

use grizzly_agent_core::{
    Agent, Completion, CompletionRequest, Content, Message, Model, Role, ScriptedProvider,
    ScriptedResponse, StopReason, SystemSection, ToolSet, ToolUse, Usage,
};
use grizzly_agent_skills::{
    SkillIndex, SkillState, SkillsSection, activate_skill_tool, deactivate_skill_tool,
};
use tokio_util::sync::CancellationToken;

fn tool_use_completion(id: &str, name: &str, input: serde_json::Value) -> Completion {
    Completion {
        content: vec![Content::ToolUse(ToolUse {
            id: id.to_owned(),
            name: name.to_owned(),
            input,
        })],
        usage: Usage {
            input_tokens: Some(10),
            output_tokens: Some(2),
        },
        stop_reason: StopReason::ToolUse,
        raw_stop_reason: "tool_use".to_owned(),
        model: "scripted-model".to_owned(),
    }
}

fn text_completion(text: &str) -> Completion {
    Completion {
        content: vec![Content::Text(text.to_owned())],
        usage: Usage {
            input_tokens: Some(5),
            output_tokens: Some(1),
        },
        stop_reason: StopReason::EndOfTurn,
        raw_stop_reason: "stop".to_owned(),
        model: "scripted-model".to_owned(),
    }
}

/// The request's system message text, or `None` if it does not open with
/// exactly one.
fn system_text(request: &CompletionRequest) -> Option<&str> {
    let Some(Message {
        role: Role::System,
        content,
    }) = request.messages.first()
    else {
        return None;
    };
    match content.as_slice() {
        [Content::Text(text)] => Some(text.as_str()),
        _ => None,
    }
}

#[tokio::test]
async fn activating_and_deactivating_a_skill_changes_the_next_rounds_system_prompt() {
    let root = tempfile::tempdir().expect("must create a tempdir");
    let skill_dir = root.path().join("frobnicate");
    std::fs::create_dir_all(&skill_dir).expect("must create the skill directory");
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: frobnicate\ndescription: frobnicates things.\n---\n\n\
         Do the frobnicating carefully.\n",
    )
    .expect("must write the fixture skill");

    let (index, diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;
    assert!(diagnostics.is_empty(), "fixture skill must parse cleanly");

    let state = Arc::new(SkillState::new(index));
    let tools = ToolSet::new([
        activate_skill_tool(Arc::clone(&state)),
        deactivate_skill_tool(Arc::clone(&state)),
    ])
    .expect("the two skill tools must not collide");

    let provider = ScriptedProvider::new(vec![
        ScriptedResponse::Completion(tool_use_completion(
            "call-1",
            "activate_skill",
            serde_json::json!({"name": "frobnicate"}),
        )),
        ScriptedResponse::Completion(tool_use_completion(
            "call-2",
            "deactivate_skill",
            serde_json::json!({"name": "frobnicate"}),
        )),
        ScriptedResponse::Completion(text_completion("all done")),
    ]);
    let model = Model::builder(Arc::new(provider.clone()), "scripted-model").build();

    let agent = Agent::builder(model, tools)
        .section(SystemSection::dynamic(SkillsSection::new(Arc::clone(
            &state,
        ))))
        .build();

    let record = agent
        .run(
            vec![Message::user("please help")],
            CancellationToken::new(),
            None,
        )
        .await
        .expect("a scripted run with no failures must succeed");
    assert_eq!(record.reply.as_deref(), Some("all done"));

    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "one request per scripted round");

    let before_activation = requests.first().expect("round 1's request");
    let after_activation = requests.get(1).expect("round 2's request");
    let after_deactivation = requests.get(2).expect("round 3's request");

    assert!(
        !system_text(before_activation)
            .expect("round 1 must open with a system message")
            .contains("Do the frobnicating carefully."),
        "no skill is active before the first round"
    );
    assert!(
        system_text(after_activation)
            .expect("round 2 must open with a system message")
            .contains("Do the frobnicating carefully."),
        "the round after activation must carry the skill's body"
    );
    assert!(
        !system_text(after_deactivation)
            .expect("round 3 must open with a system message")
            .contains("Do the frobnicating carefully."),
        "the round after deactivation must no longer carry the skill's body"
    );
}
