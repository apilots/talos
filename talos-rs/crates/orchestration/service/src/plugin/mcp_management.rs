use super::{service::McpContext, PluginManagerService};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

impl PluginManagerService {
    pub(super) fn mcp_context(&self) -> TalosResult<&McpContext> {
        self.mcp.as_ref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "MCP management is not attached to this application.",
            )
        })
    }

    pub(super) fn prepare_install_credentials(
        &self,
        request: &mut PluginInstallRequest,
    ) -> TalosResult<()> {
        if let PluginInstallTarget::ManualMcpServer {
            server,
            credentials,
        } = &mut request.target
        {
            super::credential_input::extract_inline(server, credentials)?;
            let (id, prepared) = self.manager.prepare_manual_mcp((**server).clone())?;
            **server = prepared;
            let owner = McpDefinitionRef::Plugin {
                install_id: id.clone(),
            };
            if let Some(values) = credentials.take() {
                server.credentials = Some(
                    self.mcp_context()?
                        .credentials
                        .create(&owner, server, values)?,
                );
            } else if server.credentials.is_some() {
                let previous = self.manager.read_mcp_definition(&id)?;
                if previous.credentials == server.credentials
                    && previous.transport != server.transport
                    && self
                        .mcp_context()?
                        .credentials
                        .resolve(&owner, &previous)?
                        .is_none()
                {
                    server.credentials = None;
                } else if previous.credentials != server.credentials
                    || previous.transport != server.transport
                {
                    return Err(TalosError::new(ErrorCode::PermissionDenied, "Existing MCP credentials can only be retained for the same definition and target."));
                }
            }
        }
        Ok(())
    }

    pub(super) fn definition(
        &self,
        reference: &McpDefinitionRef,
    ) -> TalosResult<McpServerDefinition> {
        let McpDefinitionRef::Plugin { install_id } = reference;
        self.manager.read_mcp_definition(install_id)
    }

    pub(super) fn authorization_definition(
        &self,
        owner: &McpDefinitionRef,
    ) -> TalosResult<McpServerDefinition> {
        let McpDefinitionRef::Plugin { install_id } = owner;
        let enabled = self.manager.mcp_servers()?.servers.iter().any(|server| {
            matches!(&server.source, McpServerSource::Plugin { install_id: candidate, .. } if candidate == install_id)
        });
        if !enabled {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "MCP authorization requires an enabled service.",
            ));
        }
        self.definition(owner)
    }

    /// Called after commit/rollback, outside the Plugin file lock. Invalid state retains its secrets.
    pub fn recover_credentials(&self) {
        if let Some(context) = &self.mcp {
            context.oauth.invalidate(|owner, captured| {
                self.authorization_definition(owner)
                    .is_ok_and(|current| current == *captured)
            });
            let result = context.credentials.recover(|owner, reference| {
                let McpDefinitionRef::Plugin { install_id } = owner;
                match self.manager.read_mcp_definition(install_id) {
                    Ok(server) => references(server.credentials.as_ref(), reference),
                    Err(error) if error.error_code() == ErrorCode::ResourceNotFound => Ok(false),
                    Err(error) => Err(error),
                }
            });
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
        }
    }

    pub fn read_mcp_authentication(
        &self,
        request: McpAuthenticationReadRequest,
    ) -> TalosResult<McpAuthenticationResponse> {
        let server = self.definition(&request.server)?;
        if let Some(response) = self.mcp_context()?.oauth.response(&request.server, &server) {
            return Ok(response);
        }
        let authentication = server
            .credentials
            .as_ref()
            .map(|binding| binding.authentication.clone())
            .unwrap_or(McpAuthentication::None);
        let state = if server.credentials.is_none() {
            McpAuthenticationState::Unconfigured
        } else {
            match self
                .mcp_context()?
                .credentials
                .authentication_state(&request.server, &server)
            {
                Ok(state) => state,
                Err(error) => {
                    talos_logging::report_error(&error);
                    McpAuthenticationState::Failed {
                        error: error.to_api_error(),
                    }
                }
            }
        };
        Ok(McpAuthenticationResponse {
            server: request.server,
            authentication,
            state,
        })
    }
}

fn references(binding: Option<&McpCredentialBinding>, reference: &str) -> TalosResult<bool> {
    let Some(binding) = binding else {
        return Ok(false);
    };
    let parse = |value: &str| {
        uuid::Uuid::parse_str(value).map_err(|error| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "The MCP credential reference is invalid.",
            )
            .with_source(error)
        })
    };
    Ok(parse(&binding.reference)? == parse(reference)?)
}
