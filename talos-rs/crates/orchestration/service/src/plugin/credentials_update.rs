use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

impl super::PluginManagerService {
    /// Serializes a complete credential change with Plugin/config commits. Network OAuth work is excluded.
    pub async fn update_mcp_credentials(
        &self,
        request: McpCredentialsUpdateRequest,
    ) -> TalosResult<McpAuthenticationResponse> {
        let _guard = self.commit_guard()?;
        self.mcp_context()?.oauth.cancel(&request.server);
        let result = self.commit_mcp_credentials(&request.server, request.change);
        self.recover_credentials();
        result?;
        self.read_mcp_authentication(McpAuthenticationReadRequest {
            node_id: None,
            server: request.server,
        })
    }

    fn commit_mcp_credentials(
        &self,
        owner: &McpDefinitionRef,
        change: McpCredentialsChange,
    ) -> TalosResult<()> {
        let context = self.mcp_context()?;
        let mut server = self.definition(owner)?;
        match change {
            McpCredentialsChange::Clear => {
                if let Some(binding) = &server.credentials {
                    context.runtime.revoke_credentials(owner);
                    context.credentials.remove_for(owner, &binding.reference)?;
                    return self.manager.publish_mcp_definitions();
                }
                let has_legacy = self.configured_mcp(owner)?.as_ref().is_some_and(|server| {
                    matches!(
                        &server.transport,
                        talos_config::McpTransportConfig::Http {
                            bearer_token: Some(_),
                            ..
                        }
                    )
                });
                if !has_legacy {
                    return Ok(());
                }
                context.runtime.revoke_credentials(owner);
                // Persist a missing binding so a restart cannot restore the removed inline token.
                server.credentials = Some(McpCredentialBinding {
                    reference: uuid::Uuid::new_v4().to_string(),
                    authentication: McpAuthentication::Bearer,
                });
            }
            McpCredentialsChange::Replace { credentials } => {
                remove_old_inline_values(&mut server, &credentials)?;
                server.credentials =
                    Some(context.credentials.create(owner, &server, credentials)?);
            }
        }
        self.publish_mcp_credentials(owner, server)
    }

    pub(super) fn publish_mcp_credentials(
        &self,
        owner: &McpDefinitionRef,
        server: McpServerDefinition,
    ) -> TalosResult<()> {
        let configured = self.configured_mcp(owner)?;
        match owner {
            McpDefinitionRef::Plugin { install_id } => {
                self.manager.set_mcp_credentials(install_id, server)
            }
            McpDefinitionRef::Config { .. } => {
                let previous = configured.expect("configuration owner was resolved");
                let mut next = previous.clone();
                next.credentials = server.credentials;
                match (&mut next.transport, server.transport) {
                    (
                        talos_config::McpTransportConfig::Stdio { env, .. },
                        McpTransportDefinition::Stdio { env: public, .. },
                    ) => *env = public.into_iter().collect(),
                    (
                        talos_config::McpTransportConfig::Http { bearer_token, .. },
                        McpTransportDefinition::StreamableHttp { .. },
                    ) => *bearer_token = None,
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "The MCP transport changed while credentials were prepared.",
                        ))
                    }
                }
                self.config_service
                    .as_ref()
                    .ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::ServiceUnavailable,
                            "Worker configuration publisher is unavailable",
                        )
                    })?
                    .update_mcp_credentials(&previous, &next)
            }
        }
    }
    fn configured_mcp(
        &self,
        owner: &McpDefinitionRef,
    ) -> TalosResult<Option<talos_config::McpServerConfig>> {
        let context = self.mcp_context()?;
        Ok(match owner {
            McpDefinitionRef::Config { name } => Some(
                context
                    .config
                    .read()
                    .expect("application config lock poisoned")
                    .agent
                    .kernel
                    .mcp
                    .servers
                    .iter()
                    .find(|server| server.name == *name)
                    .cloned()
                    .ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::ResourceNotFound,
                            "The configured MCP service was not found.",
                        )
                    })?,
            ),
            McpDefinitionRef::Plugin { .. } => None,
        })
    }
}

fn remove_old_inline_values(
    server: &mut McpServerDefinition,
    values: &McpCredentialValues,
) -> TalosResult<()> {
    super::credential_input::extract_inline(server, &mut None)?;
    match &mut server.transport {
        McpTransportDefinition::Stdio { env, .. } => {
            if let Some(secrets) = &values.env {
                env.retain(|name, _| !secrets.contains_key(name));
            }
        }
        McpTransportDefinition::StreamableHttp { headers, .. } => {
            headers.retain(|name, _| {
                let token_header = match &values.authentication {
                    McpAuthentication::Bearer | McpAuthentication::OAuth { .. } => {
                        name.eq_ignore_ascii_case("authorization")
                    }
                    McpAuthentication::Header { name: auth } => name.eq_ignore_ascii_case(auth),
                    McpAuthentication::None => false,
                };
                !token_header
                    && values.headers.as_ref().is_none_or(|secrets| {
                        !secrets
                            .keys()
                            .any(|secret| name.eq_ignore_ascii_case(secret))
                    })
            });
        }
    }
    Ok(())
}
