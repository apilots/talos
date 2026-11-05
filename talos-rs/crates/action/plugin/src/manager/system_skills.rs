use std::path::Path;

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::FileSystem;
use talos_skills::{builtin_automation_skills, SkillRoot, SkillScope};

use super::PluginManager;

impl PluginManager {
    /// Called only inside skill_roots' original plugin-state exclusive lock.
    /// The same root feeds management queries and Worker prompt construction.
    pub(super) fn materialize_system_skills(&self) -> TalosResult<SkillRoot> {
        let root = self.state.system_skills_dir();
        reject_symlink(&root)?;
        for skill in builtin_automation_skills() {
            let directory = root.join(skill.mode.skill_name());
            let path = directory.join("SKILL.md");
            reject_symlink(&directory)?;
            reject_symlink(&path)?;
            let contents = match std::fs::read(&path) {
                Ok(contents) => Some(contents),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(TalosError::internal_with_source(
                        "Built-in Skill contents could not be read.",
                        error,
                    ));
                }
            };
            if contents.as_deref() != Some(skill.contents.as_bytes()) {
                talos_fs::std_fs().write_file(&path, skill.contents)?;
            }
        }
        Ok(SkillRoot {
            path: root,
            scope: SkillScope::System,
            source_plugin: None,
            source_install: None,
            read_only: true,
        })
    }
}

fn reject_symlink(path: &Path) -> TalosResult<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Built-in Skill paths cannot be symbolic links",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TalosError::internal_with_source(
            "Built-in Skill paths could not be inspected.",
            error,
        )),
    }
}

#[cfg(test)]
#[path = "system_skills_tests.rs"]
mod tests;
