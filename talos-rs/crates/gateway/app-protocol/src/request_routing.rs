//! Typed product selectors and transfer lanes shared by every transport.
use super::ClientRequestParams;

impl ClientRequestParams {
    /// Bulk data may use the lower-priority file lane without changing API semantics.
    pub fn is_file_transfer(&self) -> bool {
        matches!(
            self,
            Self::FileUpload(_)
                | Self::FileContent(_)
                | Self::FilePreviewRead(_)
                | Self::TrajectoryReadTurn(_)
                | Self::TrajectoryReadRequest(_)
                | Self::TrajectoryReadTool(_)
                | Self::TrajectoryReadPayload(_)
        )
    }

    /// Explicit product selector. Session-only operations retain their immutable
    /// persisted binding and are never rebound from a transport header.
    pub fn computer_target(&self) -> Option<&str> {
        match self {
            Self::BuiltinJobList(request) => Some(request.computer_id.as_str()),
            Self::BuiltinJobControl(request) => Some(request.computer_id.as_str()),
            Self::SessionList(request) => request.node_id.as_deref(),
            Self::ProjectCreate(request) => request.node_id.as_deref(),
            Self::ProjectList(request) => request.node_id.as_deref(),
            Self::FsDirectoryRead(request) => request.node_id.as_deref(),
            Self::FsDirectoryCreate(request) => request.node_id.as_deref(),
            Self::TurnStart(request) => request.node_id.as_deref(),
            Self::ModelList(request) => request.node_id.as_deref(),
            Self::ConfigGet(request) => request.node_id.as_deref(),
            Self::ConfigUpdate(request) => request.node_id.as_deref(),
            Self::ConfigDelete(request) => request.node_id.as_deref(),
            Self::ConfigReload(request) => request.node_id.as_deref(),
            Self::MemoryFileList(request) => request.node_id.as_deref(),
            Self::MemoryFileRead(request) => request.node_id.as_deref(),
            Self::MemoryFileWrite(request) => request.node_id.as_deref(),
            Self::WorkspaceInfo(request) => request.node_id.as_deref(),
            Self::WorkspaceDocList(request) => request.node_id.as_deref(),
            Self::WorkspaceDocRead(request) => request.node_id.as_deref(),
            Self::WorkspaceDocWrite(request) => request.node_id.as_deref(),
            Self::SkillsList(request) => request.node_id.as_deref(),
            Self::SkillsRefresh(request) => request.node_id.as_deref(),
            Self::SkillsSuggest(request) => request.node_id.as_deref(),
            Self::SkillsRead(request) => request.node_id.as_deref(),
            Self::SkillsResolve(request) => request.node_id.as_deref(),
            Self::PluginSourceList(request) => request.node_id.as_deref(),
            Self::PluginSourceAdd(request) => request.node_id.as_deref(),
            Self::PluginSourceRefresh(request) => request.node_id.as_deref(),
            Self::PluginSourceRemove(request) => request.node_id.as_deref(),
            Self::PluginSearch(request) => request.node_id.as_deref(),
            Self::Browser(request) => request.node_id.as_deref(),
            Self::DesktopOpen(request) => Some(request.node_id.as_str()),
            Self::McpServerList(request) => request.node_id.as_deref(),
            Self::McpCredentialsUpdate(request) => request.node_id.as_deref(),
            Self::McpAuthenticationStart(request) => request.node_id.as_deref(),
            Self::McpAuthenticationRead(request) => request.node_id.as_deref(),
            Self::McpServerImport(request) => request.node_id.as_deref(),
            Self::PluginList(request) => request.node_id.as_deref(),
            Self::PluginRead(request) => request.node_id.as_deref(),
            Self::PluginInstall(request) => request.node_id.as_deref(),
            Self::PluginUpdate(request) => request.node_id.as_deref(),
            Self::PluginUninstall(request) => request.node_id.as_deref(),
            Self::PluginEnable(request) => request.node_id.as_deref(),
            Self::PluginDisable(request) => request.node_id.as_deref(),
            Self::MemorySearch(request) => request.node_id.as_deref(),
            Self::MemoryGet(request) => request.node_id.as_deref(),
            _ => None,
        }
    }

    /// Bind a typed request to the authenticated product scope. Explicit conflicting
    /// selectors are rejected before any business owner is invoked.
    pub fn bind_computer_target(&mut self, target: &str) -> talos_error::TalosResult<()> {
        self.bind_computer_target_from_alias(target, None)
    }

    /// Only a Server-authorized alias resolution may rewrite an explicit selector.
    pub fn bind_computer_target_from_alias(
        &mut self,
        target: &str,
        alias: Option<&str>,
    ) -> talos_error::TalosResult<()> {
        match self {
            Self::BuiltinJobList(request) => {
                bind_required_target(&mut request.computer_id, target, alias)
            }
            Self::BuiltinJobControl(request) => {
                bind_required_target(&mut request.computer_id, target, alias)
            }
            Self::SessionList(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::ProjectCreate(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::ProjectList(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::FsDirectoryRead(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::FsDirectoryCreate(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::TurnStart(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::ModelList(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::ConfigGet(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::ConfigUpdate(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::ConfigDelete(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::ConfigReload(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::MemoryFileList(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::MemoryFileRead(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::MemoryFileWrite(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::WorkspaceInfo(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::WorkspaceDocList(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::WorkspaceDocRead(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::WorkspaceDocWrite(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::SkillsList(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::SkillsRefresh(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::SkillsSuggest(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::SkillsRead(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::SkillsResolve(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginSourceList(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginSourceAdd(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginSourceRefresh(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginSourceRemove(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginSearch(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::Browser(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::DesktopOpen(request) => bind_required_target(&mut request.node_id, target, alias),
            Self::McpServerList(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::McpCredentialsUpdate(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::McpAuthenticationStart(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::McpAuthenticationRead(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::McpServerImport(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginList(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::PluginRead(request) => bind_optional_target(&mut request.node_id, target, alias),
            Self::PluginInstall(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginUpdate(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginUninstall(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginEnable(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::PluginDisable(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::MemorySearch(request) => {
                bind_optional_target(&mut request.node_id, target, alias)
            }
            Self::MemoryGet(request) => bind_optional_target(&mut request.node_id, target, alias),
            _ => Ok(()),
        }
    }
}

fn bind_optional_target(
    value: &mut Option<String>,
    target: &str,
    alias: Option<&str>,
) -> talos_error::TalosResult<()> {
    if value
        .as_deref()
        .is_some_and(|value| value != target && value != "embedded" && Some(value) != alias)
    {
        return Err(talos_error::TalosError::authorization_denied());
    }
    *value = Some(target.into());
    Ok(())
}

fn bind_required_target(
    value: &mut String,
    target: &str,
    alias: Option<&str>,
) -> talos_error::TalosResult<()> {
    if value != target && value != "embedded" && Some(value.as_str()) != alias {
        return Err(talos_error::TalosError::authorization_denied());
    }
    *value = target.into();
    Ok(())
}
