use super::*;
pub(super) fn encode_mcp_credential_binding(
    value: d::McpCredentialBinding,
) -> TalosResult<pb::McpCredentialBinding> {
    Ok(pb::McpCredentialBinding {
        reference: value.reference,
        authentication: Some(encode_mcp_authentication(value.authentication)?),
    })
}
pub(super) fn decode_mcp_credential_binding(
    value: pb::McpCredentialBinding,
) -> TalosResult<d::McpCredentialBinding> {
    Ok(d::McpCredentialBinding {
        reference: value.reference,
        authentication: decode_mcp_authentication(value.authentication.ok_or_else(invalid)?)?,
    })
}
pub(super) fn encode_mcp_credential_values(
    value: d::McpCredentialValues,
) -> TalosResult<pb::McpCredentialValues> {
    Ok(pb::McpCredentialValues {
        authentication: Some(encode_mcp_authentication(value.authentication)?),
        token: value
            .token
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        env: value.env.map(|value| pb::OptionalStringMap {
            values: value.into_iter().collect(),
        }),
        headers: value.headers.map(|value| pb::OptionalStringMap {
            values: value.into_iter().collect(),
        }),
    })
}
pub(super) fn decode_mcp_credential_values(
    value: pb::McpCredentialValues,
) -> TalosResult<d::McpCredentialValues> {
    Ok(d::McpCredentialValues {
        authentication: decode_mcp_authentication(value.authentication.ok_or_else(invalid)?)?,
        token: value
            .token
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        env: value.env.map(|value| value.values.into_iter().collect()),
        headers: value
            .headers
            .map(|value| value.values.into_iter().collect()),
    })
}
pub(super) fn encode_mcp_credentials_change(
    value: d::McpCredentialsChange,
) -> TalosResult<pb::McpCredentialsChange> {
    Ok(pb::McpCredentialsChange {
        value: Some(match value {
            d::McpCredentialsChange::Replace { credentials } => {
                pb::mcp_credentials_change::Value::Replace(pb::McpCredentialsChangeReplace {
                    credentials: Some(encode_mcp_credential_values(credentials)?),
                })
            }
            d::McpCredentialsChange::Clear => {
                pb::mcp_credentials_change::Value::Clear(pb::Empty {})
            }
        }),
    })
}
pub(super) fn decode_mcp_credentials_change(
    value: pb::McpCredentialsChange,
) -> TalosResult<d::McpCredentialsChange> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_credentials_change::Value::Replace(value) => d::McpCredentialsChange::Replace {
            credentials: decode_mcp_credential_values(value.credentials.ok_or_else(invalid)?)?,
        },
        pb::mcp_credentials_change::Value::Clear(_) => d::McpCredentialsChange::Clear,
    })
}
pub(super) fn encode_mcp_credentials_update_request(
    value: d::McpCredentialsUpdateRequest,
) -> TalosResult<pb::McpCredentialsUpdateParams> {
    Ok(pb::McpCredentialsUpdateParams {
        server: Some(encode_mcp_definition_ref(value.server)?),
        change: Some(encode_mcp_credentials_change(value.change)?),
    })
}
pub(super) fn decode_mcp_credentials_update_request(
    value: pb::McpCredentialsUpdateParams,
) -> TalosResult<d::McpCredentialsUpdateRequest> {
    Ok(d::McpCredentialsUpdateRequest {
        node_id: None,
        server: decode_mcp_definition_ref(value.server.ok_or_else(invalid)?)?,
        change: decode_mcp_credentials_change(value.change.ok_or_else(invalid)?)?,
    })
}
pub(crate) fn encode_mcp_definition_ref(
    value: d::McpDefinitionRef,
) -> TalosResult<pb::McpDefinitionRef> {
    Ok(pb::McpDefinitionRef {
        value: Some(match value {
            d::McpDefinitionRef::Plugin { install_id } => {
                pb::mcp_definition_ref::Value::Plugin(pb::McpDefinitionRefPlugin {
                    install_id: install_id.0,
                })
            }
            d::McpDefinitionRef::Config { name } => {
                pb::mcp_definition_ref::Value::Config(pb::McpDefinitionRefConfig { name })
            }
        }),
    })
}
pub(crate) fn decode_mcp_definition_ref(
    value: pb::McpDefinitionRef,
) -> TalosResult<d::McpDefinitionRef> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_definition_ref::Value::Plugin(value) => d::McpDefinitionRef::Plugin {
            install_id: d::PluginInstallId(value.install_id),
        },
        pb::mcp_definition_ref::Value::Config(value) => {
            d::McpDefinitionRef::Config { name: value.name }
        }
    })
}
