use std::env;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use talos_error::TalosResult;

static TALOS_HOME_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TalosPaths {
    pub talos_home: PathBuf,
    pub conf_dir: PathBuf,
    pub config_file: PathBuf,
    pub env_file: PathBuf,
    pub logs_dir: PathBuf,
    pub log_file: PathBuf,
    pub data_dir: PathBuf,
    pub sqlite_db: PathBuf,
}

impl TalosPaths {
    pub fn resolve() -> Self {
        let talos_home = resolve_talos_home();
        Self::from_home(talos_home)
    }

    pub fn from_home(talos_home: PathBuf) -> Self {
        let conf_dir = talos_home.join("conf");
        let logs_dir = talos_home.join("logs");
        let data_dir = talos_home.join("data");
        Self {
            config_file: conf_dir.join("talos.yaml"),
            env_file: talos_home.join(".env"),
            log_file: logs_dir.join("talos.log"),
            sqlite_db: data_dir.join("talos.db"),
            talos_home,
            conf_dir,
            logs_dir,
            data_dir,
        }
    }

    pub fn workspace_dir_for_agent(&self, agent_name: &str) -> PathBuf {
        self.talos_home
            .join(format!("workspace-{}", sanitize_agent_name(agent_name)))
    }

    pub fn sessions_dir_for_agent(&self, agent_name: &str) -> PathBuf {
        self.workspace_dir_for_agent(agent_name).join("sessions")
    }

    pub fn memory_dir_for_agent(&self, agent_name: &str) -> PathBuf {
        self.workspace_dir_for_agent(agent_name).join("memory")
    }

    pub fn ensure_base_dirs(&self) -> TalosResult<()> {
        use talos_error::TalosError;

        for dir in [
            &self.talos_home,
            &self.conf_dir,
            &self.logs_dir,
            &self.data_dir,
        ] {
            std::fs::create_dir_all(dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "create talos runtime dir {}: {}",
                    dir.display(),
                    err
                ))
            })?;
        }
        Ok(())
    }

    pub fn ensure_agent_workspace(&self, agent_name: &str) -> TalosResult<()> {
        use talos_error::TalosError;

        let workspace_dir = self.workspace_dir_for_agent(agent_name);
        std::fs::create_dir_all(&workspace_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "create agent workspace dir {}: {}",
                workspace_dir.display(),
                err
            ))
        })?;
        for dir in [workspace_dir.join("users").join("anonymous")] {
            std::fs::create_dir_all(&dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "create agent workspace dir {}: {}",
                    dir.display(),
                    err
                ))
            })?;
        }
        Ok(())
    }
}

pub fn set_talos_home(path: impl Into<PathBuf>) -> TalosResult<()> {
    use talos_error::TalosError;

    TALOS_HOME_OVERRIDE.set(path.into()).map_err(|_| {
        TalosError::InvalidConfiguration("talos_home is already initialized".to_string())
    })
}

pub fn resolve_talos_home() -> PathBuf {
    if let Some(path) = env::var_os("TALOS_HOME") {
        return normalize_path(PathBuf::from(path));
    }

    if let Some(path) = TALOS_HOME_OVERRIDE.get() {
        return normalize_path(path.clone());
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    normalize_path(home.join(".talos"))
}

pub fn resolve_runtime_path(path: &str) -> PathBuf {
    normalize_path(PathBuf::from(path.trim()))
}

pub fn sanitize_agent_name(agent_name: &str) -> String {
    let trimmed = agent_name.trim().to_ascii_lowercase();
    let mut out = String::with_capacity(trimmed.len());
    let mut prev_dash = false;

    for ch in trimmed.chars() {
        let safe = if ch.is_ascii_alphanumeric() { ch } else { '-' };
        if safe == '-' {
            if !prev_dash {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(safe);
            prev_dash = false;
        }
    }

    let sanitized = out.trim_matches('-');
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized.to_string()
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    if let Some(stripped) = path.to_string_lossy().strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if path == Path::new("~") {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_agent_workspace_does_not_migrate_prefix_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let paths = TalosPaths::from_home(temp_dir.path().join(".talos"));
        let workspace_dir = paths.workspace_dir_for_agent("talos");
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
        let memory_file = workspace_dir.join("MEMORY.md");
        std::fs::write(&memory_file, "# Prefix template").expect("prefix file should be written");

        paths
            .ensure_agent_workspace("talos")
            .expect("workspace dirs should be ensured");

        assert!(memory_file.exists());
        assert!(workspace_dir.join("users").join("anonymous").is_dir());
        assert!(!workspace_dir
            .join("users")
            .join("anonymous")
            .join("MEMORY.md")
            .exists());
    }
}
