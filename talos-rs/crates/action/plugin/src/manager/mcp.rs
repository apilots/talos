use super::PluginManager;
use crate::{installer::installed_mcp_servers, state::PluginInstallsFile};
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};
use talos_types::{
    McpServerRuntimeDescriptor, PluginInstall, PluginInstallId, PluginKind, PluginWarning,
    PluginWarningCode,
};

/// Prepares application-owned MCP publication before the installation state is
/// persisted. Called under the plugin file lock; must not re-enter PluginManager
/// or perform MCP I/O. The returned closure publishes without failure after save.
pub type McpChangeHook = Arc<
    dyn Fn(Vec<McpServerRuntimeDescriptor>) -> TalosResult<Box<dyn FnOnce() + Send>> + Send + Sync,
>;

/// Partial discovery: damaged manifests remain manageable as installed plugins.
pub struct McpDiscovery {
    pub servers: Vec<McpServerRuntimeDescriptor>,
    pub warnings: Vec<PluginWarning>,
}

impl PluginManager {
    /// Re-publishes committed definitions after private credential state changes.
    pub fn publish_mcp_definitions(&self) -> TalosResult<()> {
        self.state.with_exclusive_lock("plugin.mcp_publish", || {
            if let Some(prepare) = &self.mcp_change {
                let servers = discover_mcp(&self.state.load_installs()?.installs, None)?.servers;
                prepare(servers)?();
            }
            Ok(())
        })
    }

    /// Changes only credential bindings and secret-bearing maps; preserves installed programs and source identity.
    pub fn set_mcp_credentials(
        &self,
        id: &PluginInstallId,
        server: talos_types::McpServerDefinition,
    ) -> TalosResult<()> {
        self.state
            .with_exclusive_lock("plugin.mcp_credentials", || {
                let previous = self.state.load_installs()?;
                let install = previous
                    .installs
                    .iter()
                    .find(|install| install.id == *id && install.kind == PluginKind::McpServer)
                    .ok_or_else(|| {
                        TalosError::new(
                            talos_error::ErrorCode::ResourceNotFound,
                            "The MCP installation was not found.",
                        )
                    })?;
                let existing = crate::scanner::mcp_manifest::server_from_installed_mcp_manifest(
                    &install.installed_path,
                )?;
                if install.plugin_id.0 == talos_types::OFFICIAL_BROWSER_PLUGIN_ID {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "The built-in Browser does not accept MCP credential or program edits.",
                    ));
                }
                let target_matches = match (&existing.transport, &server.transport) {
                    (
                        talos_types::McpTransportDefinition::Stdio {
                            command, args, cwd, ..
                        },
                        talos_types::McpTransportDefinition::Stdio {
                            command: next_command,
                            args: next_args,
                            cwd: next_cwd,
                            ..
                        },
                    ) => command == next_command && args == next_args && cwd == next_cwd,
                    (
                        talos_types::McpTransportDefinition::StreamableHttp { url, .. },
                        talos_types::McpTransportDefinition::StreamableHttp {
                            url: next_url, ..
                        },
                    ) => url == next_url,
                    _ => false,
                };
                if existing.name != server.name || !target_matches {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "The MCP definition changed while credentials were prepared.",
                    ));
                }
                let mut plugin = crate::installer::manual_mcp_plugin(existing);
                plugin.id = install.plugin_id.clone();
                plugin.metadata.name = install.name.clone();
                plugin.definition =
                    talos_types::PluginDefinition::McpServer(talos_types::McpServerPlugin {
                        server,
                        manifest_path: "talos-mcp.json".into(),
                    });
                let candidate = crate::installer::InstallCandidate {
                    plugin,
                    source: crate::installer::InstallSource::Installed(install.clone()),
                    prepared_dir: None,
                };
                let activation = crate::installer::install_plugin(
                    &self.state,
                    candidate,
                    talos_types::PluginInstallConflictPolicy::Replace,
                    install.enabled,
                )?;
                self.finish_install(previous, activation).map(|_| ())
            })
    }

    /// Resolves the managed working directory and stable installation identity before secrets are bound.
    pub fn prepare_manual_mcp(
        &self,
        mut server: talos_types::McpServerDefinition,
    ) -> TalosResult<(PluginInstallId, talos_types::McpServerDefinition)> {
        if server.name == talos_types::BROWSER_MCP_SERVER_NAME {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "This MCP name is reserved for the built-in Browser.",
            ));
        }
        let candidate = crate::installer::manual_mcp_candidate(server.clone())?;
        let path =
            crate::installer::final_install_dir(&self.state, PluginKind::McpServer, &server.name)?;
        if let talos_types::McpTransportDefinition::Stdio { cwd, .. } = &mut server.transport {
            if cwd.is_none() {
                *cwd = Some(path);
            }
        }
        Ok((
            crate::installer::install_id_for(&candidate.plugin.id),
            server,
        ))
    }

    /// Reads the committed definition, including disabled installations. Damaged state remains an error.
    pub fn read_mcp_definition(
        &self,
        id: &PluginInstallId,
    ) -> TalosResult<talos_types::McpServerDefinition> {
        self.state.with_exclusive_lock("plugin.mcp_definition", || {
            let records = self.state.load_installs()?;
            let install = records
                .installs
                .iter()
                .find(|install| install.id == *id && install.kind == PluginKind::McpServer)
                .ok_or_else(|| {
                    TalosError::new(
                        talos_error::ErrorCode::ResourceNotFound,
                        "The MCP installation was not found.",
                    )
                })?;
            crate::scanner::mcp_manifest::server_from_installed_mcp_manifest(
                &install.installed_path,
            )
        })
    }

    pub fn with_mcp_change_hook(mut self, hook: McpChangeHook) -> Self {
        self.mcp_change = Some(hook);
        self
    }

    pub fn mcp_servers(&self) -> TalosResult<McpDiscovery> {
        self.state.with_exclusive_lock("plugin.mcp_servers", || {
            self.state.cleanup_install_temps()?;
            discover_mcp(&self.state.load_installs()?.installs, None)
        })
    }

    pub(super) fn save_installs_with_mcp(
        &self,
        previous: &PluginInstallsFile,
        installs: &PluginInstallsFile,
        target: Option<&PluginInstallId>,
    ) -> Result<(), InstallSaveError> {
        let discovery =
            discover_mcp(&installs.installs, target).map_err(InstallSaveError::Rejected)?;
        let publish = self
            .mcp_change
            .as_ref()
            .map(|prepare| prepare(discovery.servers))
            .transpose()
            .map_err(InstallSaveError::Rejected)?;
        match self.state.save_installs(installs) {
            Ok(()) => {
                if let Some(publish) = publish {
                    publish();
                }
                Ok(())
            }
            Err(error) => Err(self.confirm_failed_save(previous, installs, publish, error)),
        }
    }

    // Atomic file replacement may succeed before a directory fsync fails. Under
    // the same file lock, read back the state before deciding rollback is safe.
    fn confirm_failed_save(
        &self,
        previous: &PluginInstallsFile,
        installs: &PluginInstallsFile,
        publish: Option<Box<dyn FnOnce() + Send>>,
        error: TalosError,
    ) -> InstallSaveError {
        match self.state.load_installs() {
            Ok(current) if current == *previous => InstallSaveError::Rejected(error),
            Ok(current) if current == *installs => {
                if let Some(publish) = publish { publish(); }
                InstallSaveError::Unconfirmed(TalosError::internal_with_source("The plugin state changed, but storage durability could not be confirmed. Refresh the installation before retrying.", error))
            },
            _ => InstallSaveError::Unconfirmed(TalosError::internal_with_source("The plugin commit could not be confirmed. Installation files and backups were preserved for recovery.", error)),
        }
    }
}

pub(super) fn discover_mcp(
    installs: &[PluginInstall],
    target: Option<&PluginInstallId>,
) -> TalosResult<McpDiscovery> {
    let mut servers = Vec::new();
    let mut warnings = Vec::new();
    for install in installs
        .iter()
        .filter(|install| install.kind == PluginKind::McpServer && install.enabled)
    {
        match installed_mcp_servers(install) {
            Ok(discovered) => servers.extend(discovered),
            Err(error) if target == Some(&install.id) => return Err(error),
            Err(_) => warnings.push(PluginWarning {
                code: PluginWarningCode::InvalidManifest,
                message:
                    "The installed MCP manifest could not be loaded; this server is unavailable."
                        .into(),
                kind: Some(PluginKind::McpServer),
                source_id: None,
                path: Some(install.installed_path.to_string_lossy().into_owned()),
            }),
        }
    }
    servers.sort_by(|left, right| left.server.name.cmp(&right.server.name));
    Ok(McpDiscovery { servers, warnings })
}

/// A failed write is reversible only while the original records are confirmed.
pub(super) enum InstallSaveError {
    Rejected(TalosError),
    Unconfirmed(TalosError),
}
impl InstallSaveError {
    pub(super) fn rollback_if_uncommitted(
        self,
        rollback: impl FnOnce() -> TalosResult<()>,
    ) -> TalosError {
        if matches!(self, Self::Rejected(_)) {
            if let Err(error) = rollback() {
                talos_logging::report_error(&error);
            }
        }
        self.into_error()
    }
    pub(super) fn into_error(self) -> TalosError {
        match self {
            Self::Rejected(error) | Self::Unconfirmed(error) => error,
        }
    }
}

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;
