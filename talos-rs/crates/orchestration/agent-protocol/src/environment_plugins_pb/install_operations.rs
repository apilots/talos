use super::*;
pub(super) fn encode_plugin_disable_request(
    value: d::PluginDisableRequest,
) -> TalosResult<pb::PluginDisableParams> {
    Ok(pb::PluginDisableParams {
        install_id: value.install_id.0,
    })
}
pub(super) fn decode_plugin_disable_request(
    value: pb::PluginDisableParams,
) -> TalosResult<d::PluginDisableRequest> {
    Ok(d::PluginDisableRequest {
        node_id: None,
        install_id: d::PluginInstallId(value.install_id),
    })
}
pub(super) fn encode_plugin_disable_response(
    value: d::PluginDisableResponse,
) -> TalosResult<pb::PluginDisableResult> {
    Ok(pb::PluginDisableResult {
        plugin: Some(encode_plugin(value.plugin)?),
    })
}
pub(super) fn decode_plugin_disable_response(
    value: pb::PluginDisableResult,
) -> TalosResult<d::PluginDisableResponse> {
    Ok(d::PluginDisableResponse {
        plugin: decode_plugin(value.plugin.ok_or_else(invalid)?)?,
    })
}
pub(super) fn encode_plugin_enable_request(
    value: d::PluginEnableRequest,
) -> TalosResult<pb::PluginEnableParams> {
    Ok(pb::PluginEnableParams {
        install_id: value.install_id.0,
    })
}
pub(super) fn decode_plugin_enable_request(
    value: pb::PluginEnableParams,
) -> TalosResult<d::PluginEnableRequest> {
    Ok(d::PluginEnableRequest {
        node_id: None,
        install_id: d::PluginInstallId(value.install_id),
    })
}
pub(super) fn encode_plugin_enable_response(
    value: d::PluginEnableResponse,
) -> TalosResult<pb::PluginEnableResult> {
    Ok(pb::PluginEnableResult {
        plugin: Some(encode_plugin(value.plugin)?),
    })
}
pub(super) fn decode_plugin_enable_response(
    value: pb::PluginEnableResult,
) -> TalosResult<d::PluginEnableResponse> {
    Ok(d::PluginEnableResponse {
        plugin: decode_plugin(value.plugin.ok_or_else(invalid)?)?,
    })
}
pub(super) fn encode_plugin_install_conflict_policy(
    value: d::PluginInstallConflictPolicy,
) -> TalosResult<i32> {
    Ok(match value {
        d::PluginInstallConflictPolicy::Reject => 1,
        d::PluginInstallConflictPolicy::Replace => 2,
    })
}
pub(super) fn decode_plugin_install_conflict_policy(
    value: i32,
) -> TalosResult<d::PluginInstallConflictPolicy> {
    match value {
        1 => Ok(d::PluginInstallConflictPolicy::Reject),
        2 => Ok(d::PluginInstallConflictPolicy::Replace),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_install_request(
    value: d::PluginInstallRequest,
) -> TalosResult<pb::PluginInstallParams> {
    Ok(pb::PluginInstallParams {
        target: Some(encode_plugin_install_target(value.target)?),
        enable: value
            .enable
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        conflict_policy: encode_plugin_install_conflict_policy(value.conflict_policy)?,
    })
}
pub(super) fn decode_plugin_install_request(
    value: pb::PluginInstallParams,
) -> TalosResult<d::PluginInstallRequest> {
    Ok(d::PluginInstallRequest {
        node_id: None,
        target: decode_plugin_install_target(value.target.ok_or_else(invalid)?)?,
        enable: value
            .enable
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        conflict_policy: decode_plugin_install_conflict_policy(value.conflict_policy)?,
    })
}
pub(super) fn encode_plugin_install_response(
    value: d::PluginInstallResponse,
) -> TalosResult<pb::PluginInstallResult> {
    Ok(pb::PluginInstallResult {
        plugin: Some(encode_plugin(value.plugin)?),
        install: Some(encode_plugin_install(value.install)?),
    })
}
pub(super) fn decode_plugin_install_response(
    value: pb::PluginInstallResult,
) -> TalosResult<d::PluginInstallResponse> {
    Ok(d::PluginInstallResponse {
        plugin: decode_plugin(value.plugin.ok_or_else(invalid)?)?,
        install: decode_plugin_install(value.install.ok_or_else(invalid)?)?,
    })
}
pub(super) fn encode_plugin_install_target(
    value: d::PluginInstallTarget,
) -> TalosResult<pb::PluginInstallTarget> {
    Ok(pb::PluginInstallTarget {
        value: Some(match value {
            d::PluginInstallTarget::ManualMcpServer {
                server,
                credentials,
            } => pb::plugin_install_target::Value::ManualMcpServer(
                pb::PluginInstallTargetManualMcpServer {
                    server: Some(encode_mcp_server_definition(*server)?),
                    credentials: credentials
                        .map(|value| -> TalosResult<_> { encode_mcp_credential_values(value) })
                        .transpose()?,
                },
            ),
            d::PluginInstallTarget::Plugin {
                plugin_id,
                expected_source_commit,
            } => pb::plugin_install_target::Value::Plugin(pb::PluginInstallTargetPlugin {
                plugin_id: plugin_id.0,
                expected_source_commit: expected_source_commit
                    .map(|value| -> TalosResult<_> { Ok(value) })
                    .transpose()?,
            }),
        }),
    })
}
pub(super) fn decode_plugin_install_target(
    value: pb::PluginInstallTarget,
) -> TalosResult<d::PluginInstallTarget> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::plugin_install_target::Value::ManualMcpServer(value) => {
            d::PluginInstallTarget::ManualMcpServer {
                server: Box::new(decode_mcp_server_definition(
                    value.server.ok_or_else(invalid)?,
                )?),
                credentials: value
                    .credentials
                    .map(|value| -> TalosResult<_> { decode_mcp_credential_values(value) })
                    .transpose()?,
            }
        }
        pb::plugin_install_target::Value::Plugin(value) => d::PluginInstallTarget::Plugin {
            plugin_id: d::PluginId(value.plugin_id),
            expected_source_commit: value
                .expected_source_commit
                .map(|value| -> TalosResult<_> { Ok(value) })
                .transpose()?,
        },
    })
}
pub(super) fn encode_plugin_uninstall_request(
    value: d::PluginUninstallRequest,
) -> TalosResult<pb::PluginUninstallParams> {
    Ok(pb::PluginUninstallParams {
        install_id: value.install_id.0,
    })
}
pub(super) fn decode_plugin_uninstall_request(
    value: pb::PluginUninstallParams,
) -> TalosResult<d::PluginUninstallRequest> {
    Ok(d::PluginUninstallRequest {
        node_id: None,
        install_id: d::PluginInstallId(value.install_id),
    })
}
pub(super) fn encode_plugin_uninstall_response(
    value: d::PluginUninstallResponse,
) -> TalosResult<pb::PluginUninstallResult> {
    Ok(pb::PluginUninstallResult {
        install_id: value.install_id.0,
        uninstalled: value.uninstalled,
    })
}
pub(super) fn decode_plugin_uninstall_response(
    value: pb::PluginUninstallResult,
) -> TalosResult<d::PluginUninstallResponse> {
    Ok(d::PluginUninstallResponse {
        install_id: d::PluginInstallId(value.install_id),
        uninstalled: value.uninstalled,
    })
}
pub(super) fn encode_plugin_update_conflict_policy(
    value: d::PluginUpdateConflictPolicy,
) -> TalosResult<i32> {
    Ok(match value {
        d::PluginUpdateConflictPolicy::RejectLocalChanges => 1,
        d::PluginUpdateConflictPolicy::ReplaceLocalChanges => 2,
    })
}
pub(super) fn decode_plugin_update_conflict_policy(
    value: i32,
) -> TalosResult<d::PluginUpdateConflictPolicy> {
    match value {
        1 => Ok(d::PluginUpdateConflictPolicy::RejectLocalChanges),
        2 => Ok(d::PluginUpdateConflictPolicy::ReplaceLocalChanges),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_update_request(
    value: d::PluginUpdateRequest,
) -> TalosResult<pb::PluginUpdateParams> {
    Ok(pb::PluginUpdateParams {
        install_id: value.install_id.0,
        expected_source_commit: value
            .expected_source_commit
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        conflict_policy: encode_plugin_update_conflict_policy(value.conflict_policy)?,
    })
}
pub(super) fn decode_plugin_update_request(
    value: pb::PluginUpdateParams,
) -> TalosResult<d::PluginUpdateRequest> {
    Ok(d::PluginUpdateRequest {
        node_id: None,
        install_id: d::PluginInstallId(value.install_id),
        expected_source_commit: value
            .expected_source_commit
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        conflict_policy: decode_plugin_update_conflict_policy(value.conflict_policy)?,
    })
}
pub(super) fn encode_plugin_update_response(
    value: d::PluginUpdateResponse,
) -> TalosResult<pb::PluginUpdateResult> {
    Ok(pb::PluginUpdateResult {
        plugin: Some(encode_plugin(value.plugin)?),
        install: Some(encode_plugin_install(value.install)?),
    })
}
pub(super) fn decode_plugin_update_response(
    value: pb::PluginUpdateResult,
) -> TalosResult<d::PluginUpdateResponse> {
    Ok(d::PluginUpdateResponse {
        plugin: decode_plugin(value.plugin.ok_or_else(invalid)?)?,
        install: decode_plugin_install(value.install.ok_or_else(invalid)?)?,
    })
}
