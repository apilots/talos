use std::fs;
use std::path::Path;
use talos_error::{TalosError, TalosResult};
pub use talos_types::skills::{PluginSkillStateKey, SkillState};

pub fn load_skill_state(path: &Path) -> TalosResult<SkillState> {
    if !path.exists() {
        return Ok(SkillState::default());
    }
    let contents = fs::read_to_string(path).map_err(|err| {
        let diagnostic = format!("failed to read skills state `{}`: {err}", path.display());
        TalosError::internal_with_source(
            "Failed to read skills state; technical details were withheld.",
            err,
        )
        .context(diagnostic)
    })?;
    serde_json::from_str(&contents).map_err(|err| {
        let diagnostic = format!("failed to parse skills state `{}`: {err}", path.display());
        TalosError::internal_with_source(
            "Failed to parse skills state; technical details were withheld.",
            err,
        )
        .context(diagnostic)
    })
}

pub(crate) fn plugin_skill_state_key(
    install_id: Option<&str>,
    root: &Path,
    skill_md_path: &Path,
) -> Option<PluginSkillStateKey> {
    let install_id = install_id?;
    let relative_skill_md_path = skill_md_path
        .strip_prefix(root)
        .unwrap_or(skill_md_path)
        .to_path_buf();
    Some(PluginSkillStateKey {
        install_id: install_id.to_string(),
        relative_skill_md_path,
    })
}
