use std::sync::Arc;

use serde_json::{json, Value};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::ToolDefinition;
use talos_mcp::TalosMcpRuntime;
use talos_types::{
    JsonContentValue, McpCallToolRequest, McpContent, McpReadResourceRequest, McpResourceContents,
    McpResourceDescriptor, McpResourceTemplateDescriptor, McpServerSnapshot, TalosContentItem,
};

use crate::context::ToolContext;
use crate::output::ToolOutputPayload;
use crate::spec::{
    default_governance_for_source, RegisteredToolSpec, ToolBehaviorFlags, ToolParallelism,
    ToolPayloadKind, ToolSource,
};

pub use talos_mcp::{mcp_tool_name, split_mcp_tool_name};

#[derive(Clone)]
pub struct McpConnectionManager {
    runtime: Arc<TalosMcpRuntime>,
    snapshots: Vec<McpServerSnapshot>,
    tool_specs: Vec<RegisteredToolSpec>,
}

impl McpConnectionManager {
    pub async fn from_runtime(runtime: Arc<TalosMcpRuntime>) -> Self {
        let snapshots = runtime.snapshots().await;
        let tool_specs = specs_from_snapshots(&snapshots);
        Self {
            runtime,
            snapshots,
            tool_specs,
        }
    }

    pub fn all_tool_specs(&self) -> Vec<RegisteredToolSpec> {
        self.tool_specs.clone()
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: Option<Value>,
    ) -> TalosResult<ToolOutputPayload> {
        let response = self
            .runtime
            .call_tool(McpCallToolRequest {
                server_name: server.to_string(),
                tool_name: tool.to_string(),
                arguments: arguments.unwrap_or(Value::Null),
            })
            .await?;
        let payload = mcp_content_to_payload(&response.content)?;
        if response.is_error {
            Ok(ToolOutputPayload::Mcp {
                content: payload.text,
                is_error: true,
            })
        } else {
            Ok(ToolOutputPayload::success_with_content_items(
                json!({ "content": payload.entries }),
                payload.content_items,
            ))
        }
    }

    pub async fn list_resources(
        &self,
        server: Option<&str>,
        _cursor: Option<String>,
    ) -> TalosResult<Value> {
        let resources = self
            .snapshots
            .iter()
            .filter(|snapshot| server.is_none_or(|name| snapshot.server.name == name))
            .flat_map(|snapshot| snapshot.resources.clone())
            .collect::<Vec<_>>();
        serde_json::to_value(resources).map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize MCP resources: {err}"),
            )
        })
    }

    pub async fn list_resource_templates(
        &self,
        server: Option<&str>,
        _cursor: Option<String>,
    ) -> TalosResult<Value> {
        let templates = self
            .snapshots
            .iter()
            .filter(|snapshot| server.is_none_or(|name| snapshot.server.name == name))
            .flat_map(|snapshot| snapshot.resource_templates.clone())
            .collect::<Vec<_>>();
        serde_json::to_value(templates).map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("serialize MCP resource templates: {err}"),
            )
        })
    }

    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
        _context: &ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let response = self
            .runtime
            .read_resource(McpReadResourceRequest {
                server_name: server.to_string(),
                uri: uri.to_string(),
            })
            .await?;
        let mut content_items = Vec::new();
        let mut entries = Vec::new();
        for item in response.contents {
            match item {
                McpResourceContents::Text {
                    uri,
                    mime_type,
                    text,
                } => {
                    entries.push(json!({
                        "type": "text",
                        "uri": uri,
                        "mime_type": mime_type,
                        "text": text,
                    }));
                    content_items.push(talos_types::TalosContentItem::Text { text });
                }
                McpResourceContents::Blob {
                    uri,
                    mime_type,
                    blob,
                } => {
                    entries.push(json!({
                        "type": "blob",
                        "uri": uri,
                        "mime_type": mime_type,
                        "blob": blob,
                    }));
                }
                McpResourceContents::Unknown { raw } => entries.push(raw),
            }
        }
        Ok(ToolOutputPayload::success_with_content_items(
            json!({ "contents": entries }),
            content_items,
        ))
    }
}

fn specs_from_snapshots(snapshots: &[McpServerSnapshot]) -> Vec<RegisteredToolSpec> {
    let mut specs = Vec::new();
    for snapshot in snapshots {
        for tool in &snapshot.tools {
            let name = mcp_tool_name(&tool.server_name, &tool.name);
            let description = tool.description.clone().unwrap_or_else(|| {
                format!(
                    "MCP tool `{}` from server `{}`.",
                    tool.name, tool.server_name
                )
            });
            let spec = RegisteredToolSpec::new(
                ToolDefinition::new(name.clone(), description, Some(tool.input_schema.clone())),
                ToolSource::Mcp,
                ToolPayloadKind::Mcp,
                ToolParallelism::ParallelSafe,
                false,
                default_governance_for_source(ToolSource::Mcp),
            )
            .with_internal_name(name)
            .with_user_facing_name(format!("{}:{}", tool.server_name, tool.name))
            .with_behavior_flags(ToolBehaviorFlags {
                read_only: true,
                destructive: false,
                concurrency_safe: true,
                requires_user_interaction: false,
            });
            specs.push(spec);
        }
    }
    if !snapshots.is_empty() {
        specs.extend(resource_tool_specs());
    }
    specs
}

fn resource_tool_specs() -> Vec<RegisteredToolSpec> {
    vec![
        function_spec(
            "ListMcpResources",
            "List available MCP resources from one server or all connected servers.",
            json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "cursor": { "type": "string" }
                }
            }),
        ),
        function_spec(
            "ListMcpResourceTemplates",
            "List MCP resource templates from one server or all connected servers.",
            json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "cursor": { "type": "string" }
                }
            }),
        ),
        function_spec(
            "ReadMcpResource",
            "Read a specific MCP resource from a connected server.",
            json!({
                "type": "object",
                "properties": {
                    "server": { "type": "string" },
                    "uri": { "type": "string" }
                },
                "required": ["server", "uri"]
            }),
        ),
    ]
}

fn function_spec(name: &str, description: &str, parameters: Value) -> RegisteredToolSpec {
    RegisteredToolSpec::new(
        ToolDefinition::new(name, description, Some(parameters)),
        ToolSource::Mcp,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Mcp),
    )
}

struct McpPayloadContent {
    text: String,
    entries: Vec<Value>,
    content_items: Vec<TalosContentItem>,
}

fn mcp_content_to_payload(content: &[McpContent]) -> TalosResult<McpPayloadContent> {
    let mut parts = Vec::new();
    let mut entries = Vec::new();
    let mut content_items = Vec::new();
    for item in content {
        match item {
            McpContent::Text { text } => {
                parts.push(text.clone());
                entries.push(json!({
                    "type": "text",
                    "text": text,
                }));
                content_items.push(TalosContentItem::Text { text: text.clone() });
            }
            McpContent::Image { data, mime_type } => {
                parts.push(format!("[MCP image: {mime_type}]"));
                let entry = json!({
                    "type": "image",
                    "mime_type": mime_type,
                    "data_base64": data,
                });
                entries.push(entry.clone());
                content_items.push(TalosContentItem::Json {
                    value: JsonContentValue::from(entry),
                });
            }
            McpContent::Resource { resource } => {
                let raw = serde_json::to_value(resource).map_err(|err| {
                    TalosError::public(
                        talos_error::ErrorCode::ToolExecutionFailed,
                        format!("serialize MCP resource content: {err}"),
                    )
                })?;
                parts.push(raw.to_string());
                entries.push(json!({
                    "type": "resource",
                    "resource": raw,
                }));
                content_items.push(TalosContentItem::Json {
                    value: JsonContentValue::from(raw),
                });
            }
            McpContent::Unknown { raw } => {
                parts.push(raw.to_string());
                entries.push(raw.clone());
                content_items.push(TalosContentItem::Json {
                    value: JsonContentValue::from(raw.clone()),
                });
            }
        }
    }
    Ok(McpPayloadContent {
        text: parts.join("\n"),
        entries,
        content_items,
    })
}

#[allow(dead_code)]
fn _keep_descriptor_types(
    _: Option<McpResourceDescriptor>,
    _: Option<McpResourceTemplateDescriptor>,
) {
}
