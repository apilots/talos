use super::InstallCandidate;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    McpServerDefinition, McpTransportDefinition, Plugin, PluginDefinition, PluginId, PluginKind,
    PluginMetadata, PluginStatus,
};

/// A manual definition uses the same staging, activation and rollback as a source plugin.
pub(crate) fn manual_mcp_candidate(server: McpServerDefinition) -> TalosResult<InstallCandidate> {
    talos_types::validate_mcp_server_name(&server.name)?;
    if serde_json::to_vec(&server)
        .map_err(|error| {
            TalosError::internal_with_source("Failed to encode the MCP definition.", error)
        })?
        .len()
        > 256 * 1024
    {
        return Err(invalid("MCP input must not exceed 256 KiB."));
    }
    match &server.transport {
        McpTransportDefinition::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            if command.trim().is_empty()
                || command.contains('\0')
                || args.iter().any(|arg| arg.contains('\0'))
                || env.iter().any(|(key, value)| {
                    key.is_empty() || key.contains(['=', '\0']) || value.contains('\0')
                })
            {
                return Err(invalid(
                    "The MCP command, arguments or environment is invalid.",
                ));
            }
            if cwd.as_ref().is_some_and(|path| !path.is_absolute()) {
                return Err(invalid("A manual MCP working directory must be absolute."));
            }
        }
        McpTransportDefinition::StreamableHttp { url, headers } => {
            let url = url::Url::parse(url).map_err(|error| {
                invalid("The MCP endpoint must be an absolute HTTP or HTTPS URL.")
                    .with_source(error)
            })?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid("The MCP endpoint must use HTTP or HTTPS without user information or a fragment."));
            }
            let mut names = std::collections::HashSet::new();
            for (name, value) in headers {
                let name = http::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    invalid("The MCP HTTP header name is invalid.").with_source(error)
                })?;
                if !names.insert(name.clone())
                    || name.as_str().starts_with("mcp-")
                    || matches!(
                        name.as_str(),
                        "authorization"
                            | "host"
                            | "content-length"
                            | "content-type"
                            | "accept"
                            | "connection"
                            | "transfer-encoding"
                    )
                {
                    return Err(invalid(
                        "MCP headers contain a duplicate, credential or protocol-owned header.",
                    ));
                }
                http::HeaderValue::from_str(value).map_err(|error| {
                    invalid("The MCP HTTP header value is invalid.").with_source(error)
                })?;
            }
        }
    }
    Ok(InstallCandidate {
        plugin: manual_mcp_plugin(server),
        source: super::InstallSource::Manual,
    })
}

pub(crate) fn manual_mcp_plugin(server: McpServerDefinition) -> Plugin {
    Plugin {
        id: PluginId(format!("manual_mcp_{}", server.name)),
        kind: PluginKind::McpServer,
        metadata: PluginMetadata {
            name: server.name.clone(),
            display_name: server.display_name.clone(),
            description: server
                .description
                .clone()
                .unwrap_or_else(|| format!("MCP server {}", server.name)),
            short_description: None,
            icon: None,
            license: None,
        },
        documents: vec![crate::documents::mcp_manifest_document_ref(
            "talos-mcp.json".into(),
        )],
        source_id: None,
        path_in_source: None,
        source_commit: None,
        status: PluginStatus::NotInstalled,
        definition: PluginDefinition::McpServer(talos_types::McpServerPlugin {
            server,
            manifest_path: "talos-mcp.json".into(),
        }),
        warnings: vec![],
    }
}

fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
