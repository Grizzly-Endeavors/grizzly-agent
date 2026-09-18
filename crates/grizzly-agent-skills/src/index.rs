//! Scans an ordered list of directories for skills and builds a compact
//! index of them.
//!
//! Each subdirectory of a scanned directory that contains a `SKILL.md` is a
//! skill. Earlier directories take precedence on a name collision; later
//! duplicates, and any subdirectory whose `SKILL.md` fails to parse, are
//! reported as [`SkillDiagnostic`]s and skipped rather than failing the
//! whole scan.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::format::{Skill, SkillError, parse_skill};

/// One skill the scan kept, alongside the directory it was loaded from.
struct IndexedSkill {
    skill: Skill,
    directory: PathBuf,
}

/// A compact, queryable set of skills scanned from disk.
///
/// Built by [`SkillIndex::scan`]. Holds each skill's parsed frontmatter, not
/// its body — [`crate::SkillState`] loads a skill's body from disk fresh at
/// activation.
#[derive(Default)]
pub struct SkillIndex {
    skills: BTreeMap<String, IndexedSkill>,
}

impl SkillIndex {
    /// Scans `directories`, in order, for skills.
    ///
    /// Every subdirectory of every directory that contains a `SKILL.md` is
    /// parsed and validated. A directory that cannot be read, a duplicate
    /// skill name, and a skill that fails to parse are each reported as a
    /// [`SkillDiagnostic`] in the returned list and logged at `warn` —
    /// never fatal to the scan.
    #[must_use]
    pub async fn scan(
        directories: impl IntoIterator<Item = PathBuf>,
    ) -> (Self, Vec<SkillDiagnostic>) {
        let mut skills: BTreeMap<String, IndexedSkill> = BTreeMap::new();
        let mut diagnostics = Vec::new();

        for directory in directories {
            let skill_dirs = match list_subdirectories(&directory).await {
                Ok(skill_dirs) => skill_dirs,
                Err(source) => {
                    record(
                        &mut diagnostics,
                        SkillDiagnostic::Invalid {
                            path: directory,
                            reason: InvalidSkillReason::Io(source),
                        },
                    );
                    continue;
                }
            };

            for skill_dir in skill_dirs {
                match load_skill(&skill_dir).await {
                    Ok(None) => {}
                    Ok(Some(skill)) => {
                        insert_or_report_duplicate(&mut skills, &mut diagnostics, skill, skill_dir);
                    }
                    Err(diagnostic) => record(&mut diagnostics, diagnostic),
                }
            }
        }

        (Self { skills }, diagnostics)
    }

    /// The number of skills the scan kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the scan kept no skills.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// The parsed skill registered under `name`, if any.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name).map(|indexed| &indexed.skill)
    }

    /// Every registered skill's name, in index order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }

    /// The directory a registered skill was loaded from.
    pub(crate) fn directory(&self, name: &str) -> Option<&Path> {
        self.skills
            .get(name)
            .map(|indexed| indexed.directory.as_path())
    }

    /// A compact name-and-description listing of every registered skill, one
    /// per line — empty when the index holds no skills, so a
    /// [`grizzly_agent_core::DynamicSection`] wrapping it drops entirely.
    #[must_use]
    pub fn render(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let listing = self
            .skills
            .values()
            .map(|indexed| format!("- {}: {}", indexed.skill.name, indexed.skill.description))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Skills available. Call `activate_skill` with a skill's name to load its full \
             instructions before using it.\n{listing}"
        )
    }
}

fn insert_or_report_duplicate(
    skills: &mut BTreeMap<String, IndexedSkill>,
    diagnostics: &mut Vec<SkillDiagnostic>,
    skill: Skill,
    directory: PathBuf,
) {
    if let Some(existing) = skills.get(&skill.name) {
        record(
            diagnostics,
            SkillDiagnostic::Duplicate {
                name: skill.name,
                path: directory,
                kept_at: existing.directory.clone(),
            },
        );
        return;
    }
    let name = skill.name.clone();
    skills.insert(name, IndexedSkill { skill, directory });
}

fn record(diagnostics: &mut Vec<SkillDiagnostic>, diagnostic: SkillDiagnostic) {
    tracing::warn!(diagnostic = %diagnostic, "skipping skill");
    diagnostics.push(diagnostic);
}

async fn list_subdirectories(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut reader = tokio::fs::read_dir(directory).await?;
    let mut paths = Vec::new();
    while let Some(entry) = reader.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

/// Loads and parses `skill_dir`'s `SKILL.md`, or `Ok(None)` if the directory
/// has none — a subdirectory without one is simply not a skill, not an
/// error.
async fn load_skill(skill_dir: &Path) -> Result<Option<Skill>, SkillDiagnostic> {
    let skill_md = skill_dir.join("SKILL.md");
    let content = match tokio::fs::read_to_string(&skill_md).await {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(SkillDiagnostic::Invalid {
                path: skill_md,
                reason: InvalidSkillReason::Io(source),
            });
        }
    };

    let directory_name = skill_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default();
    parse_skill(&content, directory_name)
        .map(Some)
        .map_err(|source| SkillDiagnostic::Invalid {
            path: skill_md,
            reason: InvalidSkillReason::Format(source),
        })
}

/// Why a scan skipped one skill or one duplicate name.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SkillDiagnostic {
    /// A later directory declared a skill name an earlier one already
    /// registered; the earlier one was kept.
    #[error(
        "duplicate skill name `{name}` at {}; kept the one at {}",
        path.display(),
        kept_at.display()
    )]
    Duplicate {
        /// The name two or more skills declared.
        name: String,
        /// The directory of the duplicate that was skipped.
        path: PathBuf,
        /// The directory of the skill that was kept.
        kept_at: PathBuf,
    },
    /// A directory could not be listed, or a skill's `SKILL.md` could not be
    /// read or did not validate.
    #[error("invalid skill at {}: {reason}", path.display())]
    Invalid {
        /// The directory or file that failed.
        path: PathBuf,
        /// Why.
        #[source]
        reason: InvalidSkillReason,
    },
}

/// Why [`SkillDiagnostic::Invalid`] fired.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidSkillReason {
    /// The path could not be read.
    #[error("{0}")]
    Io(#[source] std::io::Error),
    /// The `SKILL.md` file was read but did not validate.
    #[error(transparent)]
    Format(#[from] SkillError),
}

#[cfg(test)]
#[path = "tests/index.rs"]
mod tests;
