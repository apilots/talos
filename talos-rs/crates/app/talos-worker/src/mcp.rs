use talos_config::{AgentKernelConfig, McpTransportConfig};
use talos_error::TalosResult;
use talos_mcp::{PreparedMcpUpdate, TalosMcpRuntime};
use talos_types::{McpServerRuntimeDescriptor, McpServerSource};

pub fn prepare(
    runtime: &TalosMcpRuntime,
    config: &AgentKernelConfig,
    plugins: Vec<McpServerRuntimeDescriptor>,
    credentials: &talos_service::McpCredentialStore,
) -> TalosResult<PreparedMcpUpdate> {
    let mut servers = configured_mcp_servers(config)?;
    servers.extend(plugins);
    // Validate conflicts even while disabled so enabling cannot activate an
    // ambiguous name. Disabled instances keep their empty owner for future edits.
    let inputs = servers
        .into_iter()
        .map(|descriptor| {
            let owner = match &descriptor.source {
                McpServerSource::Config => talos_types::McpDefinitionRef::Config {
                    name: descriptor.server.name.clone(),
                },
                McpServerSource::Plugin { install_id, .. } => {
                    talos_types::McpDefinitionRef::Plugin {
                        install_id: install_id.clone(),
                    }
                }
            };
            let (resolved, startup_error) = match credentials.resolve(&owner, &descriptor.server) {
                Ok(values) => (values, None),
                Err(error) => (None, Some(error)),
            };
            let resolved = if descriptor.server.credentials.is_none()
                && matches!(descriptor.source, McpServerSource::Config)
            {
                config
                    .mcp
                    .servers
                    .iter()
                    .find(|server| server.name == descriptor.server.name)
                    .and_then(|server| match &server.transport {
                        McpTransportConfig::Http {
                            bearer_token: Some(token),
                            ..
                        } => Some(talos_mcp::McpConnectionCredentials::Token(
                            talos_types::McpCredentialValues {
                                authentication: talos_types::McpAuthentication::Bearer,
                                token: Some(token.clone()),
                                env: None,
                                headers: None,
                            },
                        )),
                        _ => None,
                    })
            } else {
                resolved
            };
            talos_mcp::McpConnectionInput {
                descriptor,
                credentials: resolved,
                startup_error,
            }
        })
        .collect();
    let prepared = runtime.prepare(inputs)?;
    if config.tools.enable_mcp_tools {
        Ok(prepared)
    } else {
        runtime.prepare(vec![])
    }
}

fn configured_mcp_servers(
    kernel_config: &AgentKernelConfig,
) -> TalosResult<Vec<McpServerRuntimeDescriptor>> {
    if kernel_config.mcp.servers.iter().any(|server| {
        server.credentials.is_some()
            && matches!(
                &server.transport,
                McpTransportConfig::Http {
                    bearer_token: Some(_),
                    ..
                }
            )
    }) {
        return Err(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "MCP configuration cannot combine a credential reference with an inline bearer token.",
        ));
    }
    Ok(kernel_config
        .mcp
        .servers
        .iter()
        .map(|server| McpServerRuntimeDescriptor {
            content_revision: None,
            server: talos_service::mcp_definition_from_config(server),
            source: McpServerSource::Config,
        })
        .collect())
}
