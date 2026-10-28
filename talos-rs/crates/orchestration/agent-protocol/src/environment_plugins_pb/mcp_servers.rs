use super::*;
pub(super) fn encode_mcp_server_definition(
    value: d::McpServerDefinition,
) -> TalosResult<pb::McpServerDefinition> {
    Ok(pb::McpServerDefinition {
        credentials: value
            .credentials
            .map(|value| -> TalosResult<_> { encode_mcp_credential_binding(value) })
            .transpose()?,
        name: value.name,
        display_name: value
            .display_name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        description: value
            .description
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        transport: Some(encode_mcp_transport_definition(value.transport)?),
    })
}
pub(super) fn decode_mcp_server_definition(
    value: pb::McpServerDefinition,
) -> TalosResult<d::McpServerDefinition> {
    Ok(d::McpServerDefinition {
        credentials: value
            .credentials
            .map(|value| -> TalosResult<_> { decode_mcp_credential_binding(value) })
            .transpose()?,
        name: value.name,
        display_name: value
            .display_name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        description: value
            .description
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        transport: decode_mcp_transport_definition(value.transport.ok_or_else(invalid)?)?,
    })
}
pub(super) fn encode_mcp_server_import_request(
    value: d::McpServerImportRequest,
) -> TalosResult<pb::McpServerImportParams> {
    Ok(pb::McpServerImportParams {
        json: value.json,
        name: value
            .name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        enable: value
            .enable
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        conflict_policy: encode_plugin_install_conflict_policy(value.conflict_policy)?,
    })
}
pub(super) fn decode_mcp_server_import_request(
    value: pb::McpServerImportParams,
) -> TalosResult<d::McpServerImportRequest> {
    Ok(d::McpServerImportRequest {
        node_id: None,
        json: value.json,
        name: value
            .name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        enable: value
            .enable
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        conflict_policy: decode_plugin_install_conflict_policy(value.conflict_policy)?,
    })
}
pub(super) fn encode_mcp_server_info(value: d::McpServerInfo) -> TalosResult<pb::McpServerInfo> {
    Ok(pb::McpServerInfo {
        name: value.name,
        display_name: value
            .display_name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        source: Some(encode_mcp_server_source(value.source)?),
        transport: encode_mcp_transport_kind(value.transport)?,
        status: Some(encode_mcp_server_status(value.status)?),
        tool_count: value.tool_count.try_into().map_err(|_| invalid())?,
    })
}
pub(super) fn decode_mcp_server_info(value: pb::McpServerInfo) -> TalosResult<d::McpServerInfo> {
    Ok(d::McpServerInfo {
        name: value.name,
        display_name: value
            .display_name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        source: decode_mcp_server_source(value.source.ok_or_else(invalid)?)?,
        transport: decode_mcp_transport_kind(value.transport)?,
        status: decode_mcp_server_status(value.status.ok_or_else(invalid)?)?,
        tool_count: value.tool_count.try_into().map_err(|_| invalid())?,
    })
}
pub(super) fn encode_mcp_server_list_request(
    value: d::McpServerListRequest,
) -> TalosResult<pb::McpServerListParams> {
    Ok(pb::McpServerListParams {
        cursor: value
            .cursor
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        limit: value
            .limit
            .map(|value| -> TalosResult<_> { value.try_into().map_err(|_| invalid()) })
            .transpose()?,
    })
}
pub(super) fn decode_mcp_server_list_request(
    value: pb::McpServerListParams,
) -> TalosResult<d::McpServerListRequest> {
    Ok(d::McpServerListRequest {
        node_id: None,
        cursor: value
            .cursor
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        limit: value
            .limit
            .map(|value| -> TalosResult<_> { value.try_into().map_err(|_| invalid()) })
            .transpose()?,
    })
}
pub(super) fn encode_mcp_server_list_response(
    value: d::McpServerListResponse,
) -> TalosResult<pb::McpServerListResult> {
    Ok(pb::McpServerListResult {
        data: value
            .data
            .into_iter()
            .map(encode_mcp_server_info)
            .collect::<TalosResult<_>>()?,
        next_cursor: value
            .next_cursor
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn decode_mcp_server_list_response(
    value: pb::McpServerListResult,
) -> TalosResult<d::McpServerListResponse> {
    Ok(d::McpServerListResponse {
        data: value
            .data
            .into_iter()
            .map(decode_mcp_server_info)
            .collect::<TalosResult<_>>()?,
        next_cursor: value
            .next_cursor
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn encode_mcp_server_plugin(
    value: d::McpServerPlugin,
) -> TalosResult<pb::McpServerPlugin> {
    Ok(pb::McpServerPlugin {
        server: Some(encode_mcp_server_definition(value.server)?),
        manifest_path: value.manifest_path,
    })
}
pub(super) fn decode_mcp_server_plugin(
    value: pb::McpServerPlugin,
) -> TalosResult<d::McpServerPlugin> {
    Ok(d::McpServerPlugin {
        server: decode_mcp_server_definition(value.server.ok_or_else(invalid)?)?,
        manifest_path: value.manifest_path,
    })
}
pub(super) fn encode_mcp_server_source(
    value: d::McpServerSource,
) -> TalosResult<pb::McpServerSource> {
    Ok(pb::McpServerSource {
        value: Some(match value {
            d::McpServerSource::Plugin {
                plugin_id,
                install_id,
            } => pb::mcp_server_source::Value::Plugin(pb::McpServerSourcePlugin {
                plugin_id: plugin_id.0,
                install_id: install_id.0,
            }),
        }),
    })
}
pub(super) fn decode_mcp_server_source(
    value: pb::McpServerSource,
) -> TalosResult<d::McpServerSource> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_server_source::Value::Plugin(value) => d::McpServerSource::Plugin {
            plugin_id: d::PluginId(value.plugin_id),
            install_id: d::PluginInstallId(value.install_id),
        },
    })
}
pub(super) fn encode_mcp_server_status(
    value: d::McpServerStatus,
) -> TalosResult<pb::McpServerStatus> {
    Ok(pb::McpServerStatus {
        value: Some(match value {
            d::McpServerStatus::NotStarted => {
                pb::mcp_server_status::Value::NotStarted(pb::Empty {})
            }
            d::McpServerStatus::Starting => pb::mcp_server_status::Value::Starting(pb::Empty {}),
            d::McpServerStatus::Ready => pb::mcp_server_status::Value::Ready(pb::Empty {}),
            d::McpServerStatus::Failed { error } => {
                pb::mcp_server_status::Value::Failed(pb::McpServerStatusFailed {
                    error: Some(crate::error_pb::encode_api_error(error)),
                })
            }
            d::McpServerStatus::Exited { error } => {
                pb::mcp_server_status::Value::Exited(pb::McpServerStatusExited {
                    error: Some(crate::error_pb::encode_api_error(error)),
                })
            }
        }),
    })
}
pub(super) fn decode_mcp_server_status(
    value: pb::McpServerStatus,
) -> TalosResult<d::McpServerStatus> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_server_status::Value::NotStarted(_) => d::McpServerStatus::NotStarted,
        pb::mcp_server_status::Value::Starting(_) => d::McpServerStatus::Starting,
        pb::mcp_server_status::Value::Ready(_) => d::McpServerStatus::Ready,
        pb::mcp_server_status::Value::Failed(value) => d::McpServerStatus::Failed {
            error: crate::error_pb::decode_api_error(value.error.ok_or_else(invalid)?)?,
        },
        pb::mcp_server_status::Value::Exited(value) => d::McpServerStatus::Exited {
            error: crate::error_pb::decode_api_error(value.error.ok_or_else(invalid)?)?,
        },
    })
}
pub(super) fn encode_mcp_transport_definition(
    value: d::McpTransportDefinition,
) -> TalosResult<pb::McpTransportDefinition> {
    Ok(pb::McpTransportDefinition {
        value: Some(match value {
            d::McpTransportDefinition::Stdio {
                command,
                args,
                env,
                cwd,
            } => pb::mcp_transport_definition::Value::Stdio(pb::McpTransportDefinitionStdio {
                command,
                args: args.into_iter().map(Ok).collect::<TalosResult<_>>()?,
                env: env.into_iter().collect(),
                cwd: cwd
                    .map(|value| -> TalosResult<_> {
                        Ok(value.to_str().ok_or_else(invalid)?.to_owned())
                    })
                    .transpose()?,
            }),
            d::McpTransportDefinition::StreamableHttp { url, headers } => {
                pb::mcp_transport_definition::Value::StreamableHttp(
                    pb::McpTransportDefinitionStreamableHttp {
                        url,
                        headers: headers.into_iter().collect(),
                    },
                )
            }
        }),
    })
}
pub(super) fn decode_mcp_transport_definition(
    value: pb::McpTransportDefinition,
) -> TalosResult<d::McpTransportDefinition> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::mcp_transport_definition::Value::Stdio(value) => d::McpTransportDefinition::Stdio {
            command: value.command,
            args: value.args,
            env: value.env.into_iter().collect(),
            cwd: value
                .cwd
                .map(|value| -> TalosResult<_> { Ok(value.into()) })
                .transpose()?,
        },
        pb::mcp_transport_definition::Value::StreamableHttp(value) => {
            d::McpTransportDefinition::StreamableHttp {
                url: value.url,
                headers: value.headers.into_iter().collect(),
            }
        }
    })
}
pub(super) fn encode_mcp_transport_kind(value: d::McpTransportKind) -> TalosResult<i32> {
    Ok(match value {
        d::McpTransportKind::Stdio => 1,
        d::McpTransportKind::StreamableHttp => 2,
    })
}
pub(super) fn decode_mcp_transport_kind(value: i32) -> TalosResult<d::McpTransportKind> {
    match value {
        1 => Ok(d::McpTransportKind::Stdio),
        2 => Ok(d::McpTransportKind::StreamableHttp),
        _ => Err(invalid()),
    }
}
