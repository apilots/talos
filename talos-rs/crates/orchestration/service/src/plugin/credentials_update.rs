use talos_error::TalosResult;
use talos_types::*;

impl super::PluginManagerService {
    /// Serializes a complete credential change with Plugin commits. Network OAuth work is excluded.
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
                return Ok(());
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
        let McpDefinitionRef::Plugin { install_id } = owner;
        self.manager.set_mcp_credentials(install_id, server)
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
