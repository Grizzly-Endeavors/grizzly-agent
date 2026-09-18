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
