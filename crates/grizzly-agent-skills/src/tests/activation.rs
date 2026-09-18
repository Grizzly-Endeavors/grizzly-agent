//! Tests for [`super`]: activation loading a body from disk, deactivation
//! removing it, and the section rendering both.

use super::*;
use crate::index::SkillIndex;

async fn state_with_one_skill() -> (SkillState, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("frobnicate");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: frobnicate\ndescription: frobnicates things.\n---\n\n\
         Do the frobnicating carefully.\n",
    )
    .unwrap();

    let (index, diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;
    assert!(diagnostics.is_empty(), "fixture must parse cleanly");
    (SkillState::new(index), root)
}

#[tokio::test]
async fn render_before_any_activation_is_just_the_index_listing() {
    let (state, _root) = state_with_one_skill().await;
    let rendered = state.render();
    assert!(rendered.contains("frobnicate: frobnicates things."));
    assert!(!rendered.contains("frobnicating carefully"));
}

#[tokio::test]
async fn activation_loads_the_body_and_render_includes_it() {
    let (state, _root) = state_with_one_skill().await;
    state.activate("frobnicate").await.expect("must activate");
    let rendered = state.render();
    assert!(rendered.contains("Do the frobnicating carefully."));
}

#[tokio::test]
async fn activating_an_unknown_skill_names_the_available_ones() {
    let (state, _root) = state_with_one_skill().await;
    let error = state
        .activate("does-not-exist")
        .await
        .expect_err("must fail for an unregistered name");
    assert!(matches!(
        error,
        ActivationError::UnknownSkill { name, available }
            if name == "does-not-exist" && available == vec!["frobnicate".to_owned()]
    ));
}

#[tokio::test]
async fn deactivating_removes_the_body_from_render() {
    let (state, _root) = state_with_one_skill().await;
    state.activate("frobnicate").await.expect("must activate");
    assert!(state.deactivate("frobnicate"));
    let rendered = state.render();
    assert!(!rendered.contains("frobnicating carefully"));
}

#[tokio::test]
async fn deactivating_a_skill_that_was_not_active_is_a_harmless_no_op() {
    let (state, _root) = state_with_one_skill().await;
    assert!(!state.deactivate("frobnicate"));
}

async fn state_with_two_skills() -> (SkillState, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    for name in ["frobnicate", "defenestrate"] {
        let dir = root.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: does the {name} thing.\n---\n\nBody.\n"),
        )
        .unwrap();
    }
    let (index, diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;
    assert!(diagnostics.is_empty(), "fixtures must parse cleanly");
    (SkillState::new(index), root)
}

#[tokio::test]
async fn active_names_is_empty_before_any_activation() {
    let (state, _root) = state_with_one_skill().await;
    assert_eq!(state.active_names(), Vec::<String>::new());
    assert!(!state.is_active("frobnicate"));
}

#[tokio::test]
async fn active_names_reports_activation_order_not_alphabetical_order() {
    let (state, _root) = state_with_two_skills().await;

    state.activate("frobnicate").await.expect("must activate");
    state.activate("defenestrate").await.expect("must activate");

    assert_eq!(
        state.active_names(),
        vec!["frobnicate".to_owned(), "defenestrate".to_owned()],
        "active_names must preserve activation order, not sort alphabetically"
    );
    assert!(state.is_active("frobnicate"));
    assert!(state.is_active("defenestrate"));
    assert!(!state.is_active("does-not-exist"));
}

#[tokio::test]
async fn deactivation_removes_a_skill_from_active_names() {
    let (state, _root) = state_with_two_skills().await;
    state.activate("frobnicate").await.expect("must activate");
    state.activate("defenestrate").await.expect("must activate");

    assert!(state.deactivate("frobnicate"));

    assert_eq!(state.active_names(), vec!["defenestrate".to_owned()]);
    assert!(!state.is_active("frobnicate"));
}

#[tokio::test]
async fn reactivating_an_active_skill_keeps_its_original_position() {
    let (state, _root) = state_with_two_skills().await;
    state.activate("frobnicate").await.expect("must activate");
    state.activate("defenestrate").await.expect("must activate");

    state
        .activate("frobnicate")
        .await
        .expect("re-activation must succeed");

    assert_eq!(
        state.active_names(),
        vec!["frobnicate".to_owned(), "defenestrate".to_owned()],
        "re-activating an already-active skill must not move it to the end"
    );
}
