use talos_error::{TalosError, TalosResult};
use talos_types::{
    McpTransportDefinition, Plugin, PluginDefinition, PluginDocumentRole, PluginReadResponse,
};

pub(super) fn plugin(plugin: &mut Plugin) {
    if let PluginDefinition::McpServer(definition) = &mut plugin.definition {
        let values = match &mut definition.server.transport {
            McpTransportDefinition::Stdio { env, .. } => env,
            McpTransportDefinition::StreamableHttp { headers, .. } => headers,
        };
        for (name, value) in values {
            if super::credential_input::sensitive_name(name) {
                *value = "********".into();
            }
        }
    }
}

pub(super) fn read(mut response: PluginReadResponse) -> TalosResult<PluginReadResponse> {
    plugin(&mut response.plugin);
    if matches!(response.plugin.definition, PluginDefinition::McpServer(_)) {
        for document in &mut response.documents {
            if document.role != PluginDocumentRole::Manifest {
                continue;
            }
            let mut manifest: serde_json::Value = serde_json::from_str(&document.contents)
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "The MCP manifest cannot be safely displayed.",
                        error,
                    )
                })?;
            if let Some(transport) = manifest
                .get_mut("transport")
                .and_then(serde_json::Value::as_object_mut)
            {
                for field in ["env", "headers"] {
                    if let Some(values) = transport
                        .get_mut(field)
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        for (name, value) in values {
                            if super::credential_input::sensitive_name(name) {
                                *value = "********".into();
                            }
                        }
                    }
                }
            }
            document.contents = serde_json::to_string_pretty(&manifest).map_err(|error| {
                TalosError::internal_with_source(
                    "Failed to encode the MCP manifest preview.",
                    error,
                )
            })?;
        }
    }
    Ok(response)
}
