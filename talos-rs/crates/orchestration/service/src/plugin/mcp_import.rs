use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

// Import dialect only. Normalized definitions use the existing typed Plugin contract.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportServer {
    name: Option<String>,
    #[serde(rename = "type")]
    transport: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    credentials: Option<McpCredentialValues>,
}

impl super::PluginManagerService {
    /// Imports exactly one definition. Commands execute on the backend host; placeholders are literal.
    pub async fn import_mcp_server(
        &self,
        request: McpServerImportRequest,
    ) -> TalosResult<PluginInstallResponse> {
        if request.json.len() > 256 * 1024 {
            return Err(invalid("The MCP import exceeds 256 KiB."));
        }
        let mut value: serde_json::Value = serde_json::from_str(&request.json)
            .map_err(|error| invalid("The MCP import is not valid JSON.").with_source(error))?;
        let mut name = request.name;
        if let Some(map) = value
            .as_object_mut()
            .filter(|map| map.contains_key("mcpServers"))
        {
            if map.len() != 1 {
                return Err(invalid(
                    "An MCP import wrapper must contain only mcpServers.",
                ));
            }
            let wrapped = map.remove("mcpServers").expect("checked wrapper");
            let servers = wrapped
                .as_object()
                .cloned()
                .ok_or_else(|| invalid("mcpServers must be an object containing one server."))?;
            if servers.len() != 1 {
                return Err(invalid("Import exactly one MCP server at a time."));
            }
            let (key, server) = servers.into_iter().next().expect("one server");
            merge_name(&mut name, key)?;
            value = server;
        }
        let imported: ImportServer = serde_json::from_value(value).map_err(|error| {
            invalid("The MCP import contains invalid or unsupported fields.").with_source(error)
        })?;
        if let Some(inline) = imported.name {
            merge_name(&mut name, inline)?;
        }
        let name = name.ok_or_else(|| invalid("An unnamed MCP server requires a name."))?;
        let transport = match (imported.command, imported.url, imported.transport.as_deref()) {
            (Some(command), None, None | Some("stdio")) if imported.headers.is_empty() => McpTransportDefinition::Stdio { command, args: imported.args, env: imported.env, cwd: imported.cwd },
            (None, Some(url), None | Some("http" | "streamable_http")) if imported.args.is_empty() && imported.env.is_empty() && imported.cwd.is_none() => McpTransportDefinition::StreamableHttp { url, headers: imported.headers },
            _ => return Err(invalid("Use either stdio command fields or Streamable HTTP URL fields. Legacy SSE is unsupported.")),
        };
        self.install(PluginInstallRequest {
            target: PluginInstallTarget::ManualMcpServer {
                server: Box::new(McpServerDefinition {
                    name,
                    display_name: None,
                    description: None,
                    transport,
                    credentials: None,
                }),
                credentials: imported.credentials,
            },
            enable: request.enable,
            conflict_policy: request.conflict_policy,
        })
        .await
    }
}

fn merge_name(name: &mut Option<String>, next: String) -> TalosResult<()> {
    if name.as_ref().is_some_and(|current| current != &next) {
        return Err(invalid("MCP import names conflict."));
    }
    *name = Some(next);
    Ok(())
}
fn invalid(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}
