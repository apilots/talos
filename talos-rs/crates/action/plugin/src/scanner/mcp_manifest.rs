use crate::documents::{mcp_manifest_document_ref, readme_document_refs, relative_path};
use crate::source::plugin_id_for;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    MarketplaceSource, McpServerDefinition, McpServerPlugin, McpTransportDefinition, Plugin,
    PluginDefinition, PluginKind, PluginMetadata, PluginStatus,
};

#[derive(Debug, Deserialize)]
struct McpManifest {
    name: String,
    description: Option<String>,
    display_name: Option<String>,
    icon: Option<String>,
    transport: McpManifestTransport,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum McpManifestTransport {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    StreamableHttp {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
    Sse,
}

pub(crate) fn plugin_from_mcp_manifest(
    source: &MarketplaceSource,
    root: &Path,
    manifest_path: &Path,
) -> TalosResult<Plugin> {
    let manifest = read_mcp_manifest(manifest_path)?;
    validate_mcp_name(&manifest.name)?;
    let transport = transport_from_manifest(manifest.transport, None)?;
    let path_in_source = manifest_path
        .parent()
        .ok_or_else(|| {
            TalosError::InternalError(format!(
                "MCP manifest `{}` has no parent directory",
                manifest_path.display()
            ))
        })
        .and_then(|path| relative_path(root, path))?;
    let manifest_path_in_source = relative_path(root, manifest_path)?;
    let name = manifest.name;
    let description = manifest
        .description
        .unwrap_or_else(|| format!("MCP server plugin `{name}`"));
    let metadata = PluginMetadata {
        name: name.clone(),
        display_name: manifest.display_name.clone(),
        description,
        short_description: None,
        icon: manifest.icon,
        license: None,
    };
    let plugin_dir = manifest_path.parent().ok_or_else(|| {
        TalosError::InternalError(format!(
            "MCP manifest `{}` has no parent directory",
            manifest_path.display()
        ))
    })?;
    let mut documents = readme_document_refs(root, plugin_dir)?;
    documents.push(mcp_manifest_document_ref(manifest_path_in_source.clone()));
    Ok(Plugin {
        id: plugin_id_for(&source.id, PluginKind::McpServer, &path_in_source),
        kind: PluginKind::McpServer,
        metadata,
        documents,
        source_id: Some(source.id.clone()),
        path_in_source: Some(path_in_source),
        source_commit: source.last_checked_commit.clone(),
        status: PluginStatus::NotInstalled,
        definition: PluginDefinition::McpServer(McpServerPlugin {
            server: McpServerDefinition {
                name,
                display_name: None,
                description: None,
                transport,
            },
            manifest_path: manifest_path_in_source,
        }),
        warnings: Vec::new(),
    })
}

pub(crate) fn server_from_installed_mcp_manifest(
    plugin_dir: &Path,
) -> TalosResult<McpServerDefinition> {
    let manifest_path = plugin_dir.join("talos-mcp.json");
    let manifest = read_mcp_manifest(&manifest_path)?;
    validate_mcp_name(&manifest.name)?;
    let transport = transport_from_manifest(manifest.transport, Some(plugin_dir))?;
    Ok(McpServerDefinition {
        name: manifest.name,
        display_name: manifest.display_name,
        description: manifest.description,
        transport,
    })
}

fn read_mcp_manifest(manifest_path: &Path) -> TalosResult<McpManifest> {
    let contents = fs::read_to_string(manifest_path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to read MCP manifest `{}`: {err}",
            manifest_path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|err| {
        TalosError::ValidationError(format!(
            "failed to parse MCP manifest `{}`: {err}",
            manifest_path.display()
        ))
    })
}

fn transport_from_manifest(
    transport: McpManifestTransport,
    installed_dir: Option<&Path>,
) -> TalosResult<McpTransportDefinition> {
    match transport {
        McpManifestTransport::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            if command.trim().is_empty() {
                return Err(TalosError::ValidationError(
                    "MCP stdio command must not be empty".to_string(),
                ));
            }
            let cwd = if let Some(plugin_dir) = installed_dir {
                Some(match cwd {
                    Some(path) if path.is_absolute() => path,
                    Some(path) => plugin_dir.join(path),
                    None => plugin_dir.to_path_buf(),
                })
            } else {
                cwd
            };
            Ok(McpTransportDefinition::Stdio {
                command,
                args,
                env,
                cwd,
            })
        }
        McpManifestTransport::StreamableHttp { url, headers }
        | McpManifestTransport::Http { url, headers } => {
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Err(TalosError::ValidationError(format!(
                    "MCP streamable HTTP URL `{url}` must use http or https"
                )));
            }
            if headers
                .keys()
                .any(|key| key.eq_ignore_ascii_case("authorization"))
            {
                return Err(TalosError::ValidationError(
                    "MCP manifest must not contain inline authorization headers".to_string(),
                ));
            }
            Ok(McpTransportDefinition::StreamableHttp { url, headers })
        }
        McpManifestTransport::Sse => Err(TalosError::ValidationError(
            "MCP SSE transport is not supported".to_string(),
        )),
    }
}

fn validate_mcp_name(name: &str) -> TalosResult<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TalosError::ValidationError(format!(
            "invalid MCP server name `{name}`"
        )));
    }
    Ok(())
}
