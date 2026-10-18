use super::{parse_yaml_root, YamlConfigStore};
use crate::{AgentNodeConfig, AgentNodeRunnerConfig};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::{FilePermissions, FileSystem, WriteOptions};

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerState {
    enrollment_pending: bool,
    #[serde(default)]
    removed_owner: Option<String>,
}

impl YamlConfigStore {
    /// Only a verified owner-facing directory deletion response establishes this state.
    pub fn removed_registration_owner(&self) -> TalosResult<Option<String>> {
        Ok(self.load_worker_state()?.removed_owner)
    }
    pub fn mark_registration_removed(&self, node_id: &str, owner: &str) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        if self
            .load_agent_node()?
            .runner
            .as_ref()
            .is_none_or(|runner| runner.node_id != node_id)
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker identity changed during registration removal.",
            ));
        }
        let mut state = self.load_worker_state()?;
        state.removed_owner = Some(owner.into());
        talos_fs::write_json_pretty(&self.worker_state_path(), &state)
    }
    /// Explicit owner intent retires the old UUID before creating a fresh enrollment.
    /// Keep the removal marker until the replacement credential is durably saved.
    pub fn retire_removed_registration(&self, owner: &str) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let state = self.load_worker_state()?;
        if state.removed_owner.as_deref() != Some(owner) {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Only the previous registration owner may reconnect this host.",
            ));
        }
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let mut nodes: AgentNodeConfig = decode_section(&root, "agent_node")?;
        let previous = nodes.runner.take();
        set_section(&mut root, "agent_node", &nodes)?;
        self.write_host_root(&root)?;
        if let Some(previous) = previous {
            talos_fs::std_fs().remove_file(&previous.credential_file)?;
        }
        Ok(())
    }

    pub fn load_sync(&self) -> TalosResult<crate::SyncConfig> {
        decode_section(&parse_yaml_root(&self.read_config_text()?)?, "sync")
    }

    /// Persist only the user sync choice under the existing configuration lock.
    pub fn save_sync_enabled(&self, enabled: bool) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let mut sync: crate::SyncConfig = decode_section(&root, "sync")?;
        sync.enabled = Some(enabled);
        set_section(&mut root, "sync", &sync)?;
        self.write_host_root(&root)
    }

    /// Read private recovery state; never infer it from a lock file's existence.
    pub fn node_enrollment_pending(&self) -> TalosResult<bool> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        Ok(self.load_worker_state()?.enrollment_pending)
    }

    pub fn load_agent_node(&self) -> TalosResult<AgentNodeConfig> {
        let root = parse_yaml_root(&self.read_config_text()?)?;
        decode_section(&root, "agent_node")
    }

    /// Persist the uncertain-outcome barrier before a non-idempotent CreateNode call.
    /// Recovery must select an existing UUID and rotate its credential, never guess by hostname.
    pub fn begin_node_enrollment(&self, expected_node_id: Option<&str>) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let root = parse_yaml_root(&self.read_config_text()?)?;
        let nodes: AgentNodeConfig = decode_section(&root, "agent_node")?;
        if nodes.runner.as_ref().map(|runner| runner.node_id.as_str()) != expected_node_id {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker identity changed during enrollment",
            ));
        }
        if self.load_worker_state()?.enrollment_pending {
            return Err(TalosError::new(ErrorCode::Conflict,
                "Worker enrollment was not confirmed. Select the existing computer to recover its credential."));
        }
        let _worker_lock = talos_fs::std_fs().with_file_lock(&self.worker_lock_path())?;
        self.save_worker_pending()
    }

    /// Clear a definitely rejected CreateNode attempt. Never use this for an uncertain network result.
    pub fn cancel_node_creation(&self) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let root = parse_yaml_root(&self.read_config_text()?)?;
        let nodes: AgentNodeConfig = decode_section(&root, "agent_node")?;
        if nodes.runner.is_some() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker identity already exists",
            ));
        }
        let _worker_lock = talos_fs::std_fs().with_file_lock(&self.worker_lock_path())?;
        self.clear_worker_state()
    }

    /// Commit UUID first, then the protected credential, then clear the barrier.
    /// A crash leaves the UUID or the barrier available for explicit credential recovery.
    pub fn save_node_identity(
        &self,
        runner: &AgentNodeRunnerConfig,
        token: &str,
    ) -> TalosResult<()> {
        let conf_dir = self
            .config_path
            .parent()
            .ok_or_else(|| TalosError::internal("Configuration directory is missing"))?;
        let parent = runner.credential_file.parent().ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Worker credential directory is missing",
            )
        })?;
        if !runner.credential_file.is_absolute()
            || parent.canonicalize().map_err(|error| {
                TalosError::internal_with_source(
                    "Resolve Worker credential directory; technical details were withheld.",
                    error,
                )
            })? != conf_dir.canonicalize().map_err(|error| {
                TalosError::internal_with_source(
                    "Resolve configuration directory; technical details were withheld.",
                    error,
                )
            })?
            || token.trim().is_empty()
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Worker credential must be nonempty and stored inside the configuration directory",
            ));
        }
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let mut nodes: AgentNodeConfig = decode_section(&root, "agent_node")?;
        if nodes.runner.as_ref().is_some_and(|saved| {
            saved.node_id != runner.node_id
                || saved.control_plane_endpoint != runner.control_plane_endpoint
        }) {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Saved worker identity belongs to another computer or control plane",
            ));
        }
        nodes.runner = Some(runner.clone());
        let _worker_lock = talos_fs::std_fs().with_file_lock(&self.worker_lock_path())?;
        self.save_worker_pending()?;
        set_section(&mut root, "agent_node", &nodes)?;
        self.write_host_root(&root)?;
        talos_fs::std_fs().write_file_with(
            &runner.credential_file,
            token,
            WriteOptions::default().with_permissions(FilePermissions::unix(0o600)),
        )?;
        self.clear_worker_state()
    }

    /// A successful lease proves that a saved credential works after an interrupted
    /// save/cleanup. Never clear recovery state for a different UUID or credential.
    pub fn complete_node_enrollment(&self, node_id: &str, credential: &str) -> TalosResult<()> {
        // Ordinary registration needs no config write access when no recovery
        // files exist. A concurrent new enrollment must keep its own marker.
        if !self.worker_state_path().try_exists().map_err(|error| {
            TalosError::internal_with_source("Inspect Worker recovery state", error)
        })? && !self
            .worker_lock_path()
            .try_exists()
            .map_err(|error| TalosError::internal_with_source("Inspect Worker state lock", error))?
        {
            return Ok(());
        }
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        self.load_worker_state()?;
        let Some(runner) = self
            .load_agent_node()?
            .runner
            .filter(|runner| runner.node_id == node_id)
        else {
            return Ok(());
        };
        let saved = std::fs::read_to_string(&runner.credential_file).map_err(|error| {
            TalosError::internal_with_source("Read saved Worker credential", error)
        })?;
        if saved.trim() != credential.trim() {
            return Ok(());
        }
        let _worker_lock = talos_fs::std_fs().with_file_lock(&self.worker_lock_path())?;
        self.clear_worker_state()
    }

    fn worker_state_path(&self) -> std::path::PathBuf {
        self.env_path.with_file_name(".worker-state.json")
    }

    fn worker_lock_path(&self) -> std::path::PathBuf {
        self.env_path.with_file_name(".worker-state.lock")
    }

    fn load_worker_state(&self) -> TalosResult<WorkerState> {
        talos_fs::read_json_or_default(&self.worker_state_path())
    }

    fn save_worker_pending(&self) -> TalosResult<()> {
        talos_fs::write_json_pretty(
            &self.worker_state_path(),
            &WorkerState {
                enrollment_pending: true,
                removed_owner: self.load_worker_state()?.removed_owner,
            },
        )
    }

    fn clear_worker_state(&self) -> TalosResult<()> {
        // Every root Worker lock acquisition is nested under the stable config
        // lock. No waiter can retain the old inode while these files are removed.
        talos_fs::std_fs().remove_file(&self.worker_state_path())?;
        talos_fs::std_fs().remove_file(&self.worker_lock_path())
    }

    fn write_host_root(&self, root: &serde_yaml::Value) -> TalosResult<()> {
        let text = serde_yaml::to_string(root).map_err(|error| {
            TalosError::internal_with_source(
                "Serialize host settings; technical details were withheld.",
                error,
            )
        })?;
        talos_fs::std_fs().write_file(&self.config_path, &text)
    }
}

fn decode_section<T: serde::de::DeserializeOwned + Default>(
    root: &serde_yaml::Value,
    key: &str,
) -> TalosResult<T> {
    root.get(key)
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_yaml::from_value(value.clone()).map_err(|error| {
                TalosError::internal_with_source(
                    "Read host configuration; technical details were withheld.",
                    error,
                )
            })
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

fn set_section<T: serde::Serialize>(
    root: &mut serde_yaml::Value,
    key: &str,
    value: &T,
) -> TalosResult<()> {
    let value = serde_yaml::to_value(value).map_err(|error| {
        TalosError::internal_with_source(
            "Serialize host configuration; technical details were withheld.",
            error,
        )
    })?;
    super::replace_top_level_section(root, key, value)
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
