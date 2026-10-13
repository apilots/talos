use super::*;
pub(super) fn encode_mcp_authentication(
    value: d::McpAuthentication,
) -> TalosResult<pb::McpAuthentication> {
    Ok(pb::McpAuthentication {
        value: Some(match value {
            d::McpAuthentication::None => pb::mcp_authentication::Value::None(pb::Empty {}),
            d::McpAuthentication::OAuth { config } => {
                pb::mcp_authentication::Value::OAuth(pb::McpAuthenticationOAuth {
                    config: Some(encode_mcp_o_auth_config(*config)?),
                })
            }
            d::McpAuthentication::Bearer => pb::mcp_authentication::Value::Bearer(pb::Empty {}),
            d::McpAuthentication::Header { name } => {
                pb::mcp_authentication::Value::Header(pb::McpAuthenticationHeader { name })
            }
        }),
    })
}
pub(super) fn decode_mcp_authentication(
    value: pb::McpAuthentication,
) -> TalosResult<d::McpAuthentication> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_authentication::Value::None(_) => d::McpAuthentication::None,
        pb::mcp_authentication::Value::OAuth(value) => d::McpAuthentication::OAuth {
            config: Box::new(decode_mcp_o_auth_config(value.config.ok_or_else(invalid)?)?),
        },
        pb::mcp_authentication::Value::Bearer(_) => d::McpAuthentication::Bearer,
        pb::mcp_authentication::Value::Header(value) => {
            d::McpAuthentication::Header { name: value.name }
        }
    })
}
pub(super) fn encode_mcp_authentication_start_request(
    value: d::McpAuthenticationStartRequest,
) -> TalosResult<pb::McpAuthenticationStartParams> {
    Ok(pb::McpAuthenticationStartParams {
        server: Some(encode_mcp_definition_ref(value.server)?),
        config: Some(encode_mcp_o_auth_config(value.config)?),
        client_secret: value
            .client_secret
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn decode_mcp_authentication_start_request(
    value: pb::McpAuthenticationStartParams,
) -> TalosResult<d::McpAuthenticationStartRequest> {
    Ok(d::McpAuthenticationStartRequest {
        node_id: None,
        server: decode_mcp_definition_ref(value.server.ok_or_else(invalid)?)?,
        config: decode_mcp_o_auth_config(value.config.ok_or_else(invalid)?)?,
        client_secret: value
            .client_secret
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn encode_mcp_authentication_start_response(
    value: d::McpAuthenticationStartResponse,
) -> TalosResult<pb::McpAuthenticationStartResult> {
    Ok(pb::McpAuthenticationStartResult {
        server: Some(encode_mcp_definition_ref(value.server)?),
        attempt_id: value.attempt_id,
        authorization_url: value.authorization_url,
        expires_at: value.expires_at,
    })
}
pub(super) fn decode_mcp_authentication_start_response(
    value: pb::McpAuthenticationStartResult,
) -> TalosResult<d::McpAuthenticationStartResponse> {
    Ok(d::McpAuthenticationStartResponse {
        server: decode_mcp_definition_ref(value.server.ok_or_else(invalid)?)?,
        attempt_id: value.attempt_id,
        authorization_url: value.authorization_url,
        expires_at: value.expires_at,
    })
}
pub(super) fn encode_mcp_o_auth_config(
    value: d::McpOAuthConfig,
) -> TalosResult<pb::McpOAuthConfig> {
    Ok(pb::McpOAuthConfig {
        client_id: value
            .client_id
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        client_metadata_url: value
            .client_metadata_url
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        callback_port: value.callback_port.map(u32::from),
        scopes: value
            .scopes
            .into_iter()
            .map(Ok)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn decode_mcp_o_auth_config(
    value: pb::McpOAuthConfig,
) -> TalosResult<d::McpOAuthConfig> {
    Ok(d::McpOAuthConfig {
        client_id: value
            .client_id
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        client_metadata_url: value
            .client_metadata_url
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        callback_port: value
            .callback_port
            .map(|value| -> TalosResult<_> { value.try_into().map_err(|_| invalid()) })
            .transpose()?,
        scopes: value
            .scopes
            .into_iter()
            .map(Ok)
            .collect::<TalosResult<_>>()?,
    })
}
