use super::{parse_yaml_root, YamlConfigStore};
use crate::{AgentNodeConfig, AgentNodeRunnerConfig, DesktopConfig, DesktopConnectionMode};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::{FilePermissions, FileSystem, WriteOptions};

impl YamlConfigStore {
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
        self.write_desktop_root(&root)
    }

    pub fn load_desktop(&self) -> TalosResult<DesktopConfig> {
        let root = parse_yaml_root(&self.read_config_text()?)?;
        decode_section(&root, "desktop")
    }

    pub fn load_agent_node(&self) -> TalosResult<AgentNodeConfig> {
        let root = parse_yaml_root(&self.read_config_text()?)?;
        decode_section(&root, "agent_node")
    }

    /// Change only Desktop connection policy; preserve worker identity and other sections.
    pub fn save_desktop_connection(&self, mode: DesktopConnectionMode) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let mut desktop: DesktopConfig = decode_section(&root, "desktop")?;
        desktop.connection_mode = mode;
        set_section(&mut root, "desktop", &desktop)?;
        self.write_desktop_root(&root)
    }

    /// Persist the uncertain-outcome barrier before a non-idempotent CreateNode call.
    /// Recovery must select an existing UUID and rotate its credential, never guess by hostname.
    pub fn begin_node_enrollment(&self, expected_node_id: Option<&str>) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let mut desktop: DesktopConfig = decode_section(&root, "desktop")?;
        let nodes: AgentNodeConfig = decode_section(&root, "agent_node")?;
        if nodes.runner.as_ref().map(|runner| runner.node_id.as_str()) != expected_node_id {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker identity changed during enrollment",
            ));
        }
        if desktop.enrollment_pending {
            return Err(TalosError::new(ErrorCode::Conflict,
                "Worker enrollment was not confirmed. Select the existing computer to recover its credential."));
        }
        desktop.enrollment_pending = true;
        set_section(&mut root, "desktop", &desktop)?;
        self.write_desktop_root(&root)
    }

    /// Clear a definitely rejected CreateNode attempt. Never use this for an uncertain network result.
    pub fn cancel_node_creation(&self) -> TalosResult<()> {
        let _lock = talos_fs::std_fs().with_file_lock(&self.lock_path())?;
        let mut root = parse_yaml_root(&self.read_config_text()?)?;
        let nodes: AgentNodeConfig = decode_section(&root, "agent_node")?;
        if nodes.runner.is_some() {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Worker identity already exists",
            ));
        }
        let mut desktop: DesktopConfig = decode_section(&root, "desktop")?;
        desktop.enrollment_pending = false;
        set_section(&mut root, "desktop", &desktop)?;
        self.write_desktop_root(&root)
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
        let mut desktop: DesktopConfig = decode_section(&root, "desktop")?;
        desktop.enrollment_pending = true;
        set_section(&mut root, "agent_node", &nodes)?;
        set_section(&mut root, "desktop", &desktop)?;
        self.write_desktop_root(&root)?;
        talos_fs::std_fs().write_file_with(
            &runner.credential_file,
            token,
            WriteOptions::default().with_permissions(FilePermissions::unix(0o600)),
        )?;
        desktop.enrollment_pending = false;
        set_section(&mut root, "desktop", &desktop)?;
        self.write_desktop_root(&root)
    }

    fn write_desktop_root(&self, root: &serde_yaml::Value) -> TalosResult<()> {
        let text = serde_yaml::to_string(root).map_err(|error| {
            TalosError::internal_with_source(
                "Serialize Desktop settings; technical details were withheld.",
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
                    "Read Desktop configuration; technical details were withheld.",
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
            "Serialize Desktop configuration; technical details were withheld.",
            error,
        )
    })?;
    super::replace_top_level_section(root, key, value)
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
