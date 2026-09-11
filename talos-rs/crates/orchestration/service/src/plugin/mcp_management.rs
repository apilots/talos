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

    pub(super) fn install_with_credentials(
        &self,
        mut request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
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
        self.manager.install(request)
    }

    pub(super) fn definition(
        &self,
        reference: &McpDefinitionRef,
    ) -> TalosResult<McpServerDefinition> {
        match reference {
            McpDefinitionRef::Plugin { install_id } => self.manager.read_mcp_definition(install_id),
            McpDefinitionRef::Config { name } => self
                .mcp_context()?
                .config
                .read()
                .expect("application config lock poisoned")
                .agent
                .kernel
                .mcp
                .servers
                .iter()
                .find(|server| server.name == *name)
                .map(mcp_definition_from_config)
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ResourceNotFound,
                        "The configured MCP service was not found.",
                    )
                }),
        }
    }

    /// Called after commit/rollback, outside the Plugin file lock. Invalid state retains its secrets.
    pub fn recover_credentials(&self) {
        if let Some(context) = &self.mcp {
            context.oauth.invalidate(|owner, captured| {
                self.definition(owner)
                    .is_ok_and(|current| current == *captured)
            });
            let active = context
                .config
                .read()
                .expect("application config lock poisoned")
                .clone();
            let persisted = active.reload();
            let result = context.credentials.recover(|owner, reference| {
                let matches = |server: &McpServerDefinition| {
                    references(server.credentials.as_ref(), reference)
                };
                match owner {
                    McpDefinitionRef::Plugin { install_id } => {
                        match self.manager.read_mcp_definition(install_id) {
                            Ok(server) => matches(&server),
                            Err(error) if error.error_code() == ErrorCode::ResourceNotFound => {
                                Ok(false)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    McpDefinitionRef::Config { name } => {
                        let contains = |config: &talos_config::Config| match config
                            .agent
                            .kernel
                            .mcp
                            .servers
                            .iter()
                            .find(|server| server.name == *name)
                        {
                            Some(server) => references(server.credentials.as_ref(), reference),
                            None => Ok(false),
                        };
                        if contains(&active)? {
                            Ok(true)
                        } else {
                            contains(persisted.as_ref().map_err(Clone::clone)?)
                        }
                    }
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
        let legacy = match &request.server {
            McpDefinitionRef::Config { name } => self
                .mcp_context()?
                .config
                .read()
                .expect("application config lock poisoned")
                .agent
                .kernel
                .mcp
                .servers
                .iter()
                .any(|server| {
                    server.name == *name
                        && matches!(
                            &server.transport,
                            talos_config::McpTransportConfig::Http {
                                bearer_token: Some(_),
                                ..
                            }
                        )
                }),
            _ => false,
        };
        let authentication = server
            .credentials
            .as_ref()
            .map(|binding| binding.authentication.clone())
            .unwrap_or(if legacy {
                McpAuthentication::Bearer
            } else {
                McpAuthentication::None
            });
        let state = if server.credentials.is_none() {
            if legacy {
                McpAuthenticationState::CredentialsStored
            } else {
                McpAuthenticationState::Unconfigured
            }
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

/// Canonical configuration conversion excludes legacy bearer values from serializable definitions.
pub fn mcp_definition_from_config(server: &talos_config::McpServerConfig) -> McpServerDefinition {
    let transport = match &server.transport {
        talos_config::McpTransportConfig::Stdio { command, args, env } => {
            McpTransportDefinition::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                cwd: None,
            }
        }
        talos_config::McpTransportConfig::Http { url, .. } => {
            McpTransportDefinition::StreamableHttp {
                url: url.clone(),
                headers: Default::default(),
            }
        }
    };
    McpServerDefinition {
        credentials: server.credentials.clone(),
        name: server.name.clone(),
        display_name: None,
        description: None,
        transport,
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
