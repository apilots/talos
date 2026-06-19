use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillState {
    #[serde(default)]
    pub disabled_paths: BTreeSet<PathBuf>,
}

impl SkillState {
    pub fn is_disabled(&self, path: &Path) -> bool {
        let key = path_key(path);
        self.disabled_paths
            .iter()
            .any(|entry| path_key(entry) == key)
    }

    pub fn disable(&mut self, path: impl Into<PathBuf>) {
        self.disabled_paths.insert(path_key(path.into()));
    }

    pub fn enable(&mut self, path: &Path) {
        let key = path_key(path);
        self.disabled_paths.retain(|entry| path_key(entry) != key);
    }
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

pub(crate) fn path_key(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
