//! Tests for [`super`]: scan precedence, duplicate reporting, and skipping
//! invalid skills without failing the whole scan.

use super::*;

/// Writes a minimal, valid `SKILL.md` under `root/name/`, creating the
/// directory. Returns that directory's path.
fn write_skill(root: &Path, name: &str, description: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\nBody.\n"),
    )
    .unwrap();
    dir
}

/// Writes a `SKILL.md` whose declared `name` does not match `dir_name`, so
/// the scan reports it as invalid.
fn write_invalid_skill(root: &Path, dir_name: &str) -> PathBuf {
    let dir = root.join(dir_name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: does-not-match\ndescription: broken.\n---\n\nBody.\n",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn scanning_no_directories_yields_an_empty_index() {
    let (index, diagnostics) = SkillIndex::scan(Vec::new()).await;
    assert!(index.is_empty());
    assert!(diagnostics.is_empty());
}

#[tokio::test]
async fn scanning_finds_a_valid_skill() {
    let root = tempfile::tempdir().unwrap();
    write_skill(root.path(), "do-thing", "does a thing.");

    let (index, diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;

    assert!(diagnostics.is_empty());
    assert_eq!(index.len(), 1);
    let skill = index.get("do-thing").expect("must be indexed");
    assert_eq!(skill.description, "does a thing.");
}

#[tokio::test]
async fn earlier_directories_take_precedence_on_a_name_collision() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    write_skill(first.path(), "shared", "from the first directory.");
    write_skill(second.path(), "shared", "from the second directory.");

    let (index, diagnostics) = SkillIndex::scan(vec![
        first.path().to_path_buf(),
        second.path().to_path_buf(),
    ])
    .await;

    assert_eq!(index.len(), 1);
    assert_eq!(
        index.get("shared").expect("must be indexed").description,
        "from the first directory."
    );
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(
        diagnostics.first(),
        Some(SkillDiagnostic::Duplicate { name, .. }) if name == "shared"
    ));
}

#[tokio::test]
async fn an_invalid_skill_is_reported_and_skipped_without_failing_the_scan() {
    let root = tempfile::tempdir().unwrap();
    write_skill(root.path(), "good", "a valid skill.");
    write_invalid_skill(root.path(), "bad");

    let (index, diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;

    assert_eq!(index.len(), 1);
    assert!(index.get("good").is_some());
    assert_eq!(diagnostics.len(), 1);
    assert!(matches!(
        diagnostics.first(),
        Some(SkillDiagnostic::Invalid { .. })
    ));
}

#[tokio::test]
async fn a_subdirectory_without_skill_md_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("scratch")).unwrap();

    let (index, diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;

    assert!(index.is_empty());
    assert!(diagnostics.is_empty());
}

#[tokio::test]
async fn render_lists_every_skills_name_and_description() {
    let root = tempfile::tempdir().unwrap();
    write_skill(root.path(), "alpha", "the first skill.");
    write_skill(root.path(), "beta", "the second skill.");

    let (index, _diagnostics) = SkillIndex::scan(vec![root.path().to_path_buf()]).await;
    let rendered = index.render();

    assert!(rendered.contains("alpha: the first skill."));
    assert!(rendered.contains("beta: the second skill."));
}

#[tokio::test]
async fn render_is_empty_for_an_empty_index() {
    let (index, _diagnostics) = SkillIndex::scan(Vec::new()).await;
    assert_eq!(index.render(), "");
}
