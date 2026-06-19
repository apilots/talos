use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillState {
    #[serde(default = "default_state_version")]
    pub version: u32,
    #[serde(default)]
    pub disabled: BTreeSet<PluginSkillStateKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PluginSkillStateKey {
    pub install_id: String,
    pub relative_skill_md_path: PathBuf,
}

impl SkillState {
    pub fn is_disabled(&self, key: Option<&PluginSkillStateKey>) -> bool {
        let Some(key) = key else {
            return false;
        };
        self.disabled.contains(key)
    }

    pub fn disable(&mut self, key: PluginSkillStateKey) {
        self.disabled.insert(key);
    }

    pub fn enable(&mut self, key: &PluginSkillStateKey) {
        self.disabled.remove(key);
    }
}

fn default_state_version() -> u32 {
    1
}

pub fn load_skill_state(path: &Path) -> TalosResult<SkillState> {
    if !path.exists() {
        return Ok(SkillState::default());
    }
    let contents = fs::read_to_string(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to read skills state `{}`: {err}",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|err| {
        TalosError::SerializationError(format!(
            "failed to parse skills state `{}`: {err}",
            path.display()
        ))
    })
}

pub fn save_skill_state(path: &Path, state: &SkillState) -> TalosResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to create skills state directory `{}`: {err}",
                parent.display()
            ))
        })?;
    }
    let contents = serde_json::to_string_pretty(state).map_err(|err| {
        TalosError::SerializationError(format!("failed to serialize skills state: {err}"))
    })?;
    fs::write(path, contents).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to write skills state `{}`: {err}",
            path.display()
        ))
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
