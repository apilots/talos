use super::PluginManager;
use crate::installer::{install_plugin, manual_mcp_candidate};
use std::path::Path;
use talos_error::{TalosError, TalosResult};
use talos_types::{
    McpServerDefinition, McpTransportDefinition, PluginId, PluginInstallConflictPolicy,
    BUILTIN_BROWSER_PLUGIN_ID, BUILTIN_BROWSER_SERVER_NAME,
};

impl PluginManager {
    /// Reads the original installation record; no separate enabled flag.
    pub fn builtin_browser_enabled(&self) -> TalosResult<bool> {
        self.state
            .with_exclusive_lock("plugin.builtin.browser.read", || {
                Ok(self.state.load_installs()?.installs.iter().any(|install| {
                    install.plugin_id.0 == BUILTIN_BROWSER_PLUGIN_ID && install.enabled
                }))
            })
    }

    /// Provision the bundled Browser through the original activation transaction.
    /// Only the composition root calls this method; user imports cannot claim its
    /// identity. Repeated startup and executable upgrades preserve explicit disable.
    pub fn provision_builtin_browser(&self, executable: &Path) -> TalosResult<()> {
        if !executable.is_absolute() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "The bundled Browser executable must be absolute.",
            ));
        }
        let mut server: McpServerDefinition = serde_json::from_str(include_str!(
            "../../resources/browser/talos-mcp.json"
        ))
        .map_err(|error| {
            TalosError::internal_with_source("The bundled Browser manifest is invalid.", error)
        })?;
        if let McpTransportDefinition::Stdio { command, .. } = &mut server.transport {
            *command = executable.to_string_lossy().into_owned();
        }
        self.state.with_exclusive_lock("plugin.builtin.browser", || {
            let previous = self.state.load_installs()?;
            let installed = previous.installs.iter()
                .find(|install| install.plugin_id.0 == BUILTIN_BROWSER_PLUGIN_ID);
            if previous.installs.iter().any(|install|
                install.name == BUILTIN_BROWSER_SERVER_NAME && install.plugin_id.0 != BUILTIN_BROWSER_PLUGIN_ID) {
                return Err(TalosError::new(talos_error::ErrorCode::Conflict,
                    "An existing plugin uses the reserved Browser name; rename it before enabling the bundled Browser."));
            }
            if let Some(install) = installed {
                let current = crate::scanner::mcp_manifest::server_from_installed_mcp_manifest(
                    &install.installed_path,
                )?;
                // Installed relative cwd is derived at read time, not persisted.
                let mut expected = server.clone();
                if let McpTransportDefinition::Stdio { cwd, .. } = &mut expected.transport {
                    *cwd = Some(install.installed_path.clone());
                }
                if current == expected { return Ok(()); }
            }
            let enabled = installed.is_none_or(|install| install.enabled);
            let mut candidate = manual_mcp_candidate(server)?;
            candidate.plugin.id = PluginId(BUILTIN_BROWSER_PLUGIN_ID.into());
            let activation = install_plugin(&self.state, candidate,
                PluginInstallConflictPolicy::Replace, enabled)?;
            self.finish_install(previous, activation).map(|_| ())
        })
    }
}
