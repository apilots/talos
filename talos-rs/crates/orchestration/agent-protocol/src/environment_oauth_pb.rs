use super::{environment_pb::invalid, environment_plugins_pb as plugins, *};
use agent_pb as pb;
use talos_error::TalosResult;

pub fn encode_callback_target(value: McpOAuthCallbackTarget) -> pb::McpOAuthCallbackTarget {
    use pb::mcp_o_auth_callback_target::Target;
    pb::McpOAuthCallbackTarget {
        target: Some(match value {
            McpOAuthCallbackTarget::Loopback => Target::Loopback(pb::Empty {}),
            McpOAuthCallbackTarget::ControlPlaneRelay { uri } => Target::ControlPlaneRelayUri(uri),
        }),
    }
}
pub fn decode_callback_target(
    value: Option<pb::McpOAuthCallbackTarget>,
) -> TalosResult<McpOAuthCallbackTarget> {
    use pb::mcp_o_auth_callback_target::Target;
    match value.and_then(|value| value.target).ok_or_else(invalid)? {
        Target::Loopback(_) => Ok(McpOAuthCallbackTarget::Loopback),
        Target::ControlPlaneRelayUri(uri) if !uri.is_empty() => {
            Ok(McpOAuthCallbackTarget::ControlPlaneRelay { uri })
        }
        Target::ControlPlaneRelayUri(_) => Err(invalid()),
    }
}
pub fn encode_complete_mcp_authentication_params(
    value: McpAuthenticationCallbackParams,
) -> TalosResult<pb::McpAuthenticationCallbackParams> {
    use pb::mcp_authentication_callback_params::Outcome;
    Ok(pb::McpAuthenticationCallbackParams {
        server: Some(plugins::encode_mcp_definition_ref(value.server)?),
        attempt_id: value.attempt_id,
        state: value.state,
        issuer: value.issuer,
        outcome: Some(match value.outcome {
            McpOAuthCallbackOutcome::AuthorizationCode(code) => Outcome::AuthorizationCode(code),
            McpOAuthCallbackOutcome::ProviderErrorCode(code) => Outcome::ProviderErrorCode(code),
        }),
    })
}
pub fn decode_complete_mcp_authentication_params(
    value: Option<pb::McpAuthenticationCallbackParams>,
) -> TalosResult<McpAuthenticationCallbackParams> {
    use pb::mcp_authentication_callback_params::Outcome;
    let value = value.ok_or_else(invalid)?;
    if value.attempt_id.is_empty()
        || value.state.is_empty()
        || value.state.len() > 4096
        || value.issuer.as_ref().is_some_and(|value| value.is_empty())
    {
        return Err(invalid());
    }
    let outcome = match value.outcome.ok_or_else(invalid)? {
        Outcome::AuthorizationCode(code) if !code.is_empty() && code.len() <= 4096 => {
            McpOAuthCallbackOutcome::AuthorizationCode(code)
        }
        Outcome::ProviderErrorCode(code) if !code.is_empty() && code.len() <= 256 => {
            McpOAuthCallbackOutcome::ProviderErrorCode(code)
        }
        _ => return Err(invalid()),
    };
    Ok(McpAuthenticationCallbackParams {
        server: plugins::decode_mcp_definition_ref(value.server.ok_or_else(invalid)?)?,
        attempt_id: value.attempt_id,
        state: value.state,
        issuer: value.issuer,
        outcome,
    })
}
pub fn encode_complete_mcp_authentication_result(
    value: talos_types::McpAuthenticationResponse,
) -> TalosResult<pb::McpAuthenticationResult> {
    plugins::encode_mcp_authentication_response(value)
}
pub fn decode_complete_mcp_authentication_result(
    value: pb::McpAuthenticationResult,
) -> TalosResult<talos_types::McpAuthenticationResponse> {
    plugins::decode_mcp_authentication_response(value)
}
