use super::*;
pub(super) fn encode_mcp_authentication_read_request(
    value: d::McpAuthenticationReadRequest,
) -> TalosResult<pb::McpAuthenticationReadParams> {
    Ok(pb::McpAuthenticationReadParams {
        server: Some(encode_mcp_definition_ref(value.server)?),
    })
}
pub(super) fn decode_mcp_authentication_read_request(
    value: pb::McpAuthenticationReadParams,
) -> TalosResult<d::McpAuthenticationReadRequest> {
    Ok(d::McpAuthenticationReadRequest {
        node_id: None,
        server: decode_mcp_definition_ref(value.server.ok_or_else(invalid)?)?,
    })
}
pub(crate) fn encode_mcp_authentication_response(
    value: d::McpAuthenticationResponse,
) -> TalosResult<pb::McpAuthenticationResult> {
    Ok(pb::McpAuthenticationResult {
        server: Some(encode_mcp_definition_ref(value.server)?),
        authentication: Some(encode_mcp_authentication(value.authentication)?),
        state: Some(encode_mcp_authentication_state(value.state)?),
    })
}
pub(crate) fn decode_mcp_authentication_response(
    value: pb::McpAuthenticationResult,
) -> TalosResult<d::McpAuthenticationResponse> {
    Ok(d::McpAuthenticationResponse {
        server: decode_mcp_definition_ref(value.server.ok_or_else(invalid)?)?,
        authentication: decode_mcp_authentication(value.authentication.ok_or_else(invalid)?)?,
        state: decode_mcp_authentication_state(value.state.ok_or_else(invalid)?)?,
    })
}
pub(super) fn encode_mcp_authentication_state(
    value: d::McpAuthenticationState,
) -> TalosResult<pb::McpAuthenticationState> {
    Ok(pb::McpAuthenticationState {
        value: Some(match value {
            d::McpAuthenticationState::Authorizing {
                attempt_id,
                expires_at,
            } => pb::mcp_authentication_state::Value::Authorizing(
                pb::McpAuthenticationStateAuthorizing {
                    attempt_id,
                    expires_at,
                },
            ),
            d::McpAuthenticationState::Authenticated => {
                pb::mcp_authentication_state::Value::Authenticated(pb::Empty {})
            }
            d::McpAuthenticationState::Expired => {
                pb::mcp_authentication_state::Value::Expired(pb::Empty {})
            }
            d::McpAuthenticationState::Unconfigured => {
                pb::mcp_authentication_state::Value::Unconfigured(pb::Empty {})
            }
            d::McpAuthenticationState::CredentialsStored => {
                pb::mcp_authentication_state::Value::CredentialsStored(pb::Empty {})
            }
            d::McpAuthenticationState::Missing => {
                pb::mcp_authentication_state::Value::Missing(pb::Empty {})
            }
            d::McpAuthenticationState::Failed { error } => {
                pb::mcp_authentication_state::Value::Failed(pb::McpAuthenticationStateFailed {
                    error: Some(crate::error_pb::encode_api_error(error)),
                })
            }
        }),
    })
}
pub(super) fn decode_mcp_authentication_state(
    value: pb::McpAuthenticationState,
) -> TalosResult<d::McpAuthenticationState> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_authentication_state::Value::Authorizing(value) => {
            d::McpAuthenticationState::Authorizing {
                attempt_id: value.attempt_id,
                expires_at: value.expires_at,
            }
        }
        pb::mcp_authentication_state::Value::Authenticated(_) => {
            d::McpAuthenticationState::Authenticated
        }
        pb::mcp_authentication_state::Value::Expired(_) => d::McpAuthenticationState::Expired,
        pb::mcp_authentication_state::Value::Unconfigured(_) => {
            d::McpAuthenticationState::Unconfigured
        }
        pb::mcp_authentication_state::Value::CredentialsStored(_) => {
            d::McpAuthenticationState::CredentialsStored
        }
        pb::mcp_authentication_state::Value::Missing(_) => d::McpAuthenticationState::Missing,
        pb::mcp_authentication_state::Value::Failed(value) => d::McpAuthenticationState::Failed {
            error: crate::error_pb::decode_api_error(value.error.ok_or_else(invalid)?)?,
        },
    })
}
