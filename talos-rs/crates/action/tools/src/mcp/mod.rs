use std::sync::Arc;
use talos_browser::BrowserActionExt;

use serde_json::{json, Value};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::ToolDefinition;
use talos_mcp::{McpConnection, TalosMcpRuntime};
use talos_types::{
    JsonContentValue, McpCallToolRequest, McpCallToolResponse, McpContent, McpReadResourceRequest,
    McpResourceContents, TalosContentItem,
};

use crate::context::ToolContext;
use crate::output::ToolOutputPayload;
use crate::spec::{
    default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind, ToolSource,
};

mod images;
mod resources;

pub use talos_mcp::{mcp_tool_name, split_mcp_tool_name};

#[cfg(test)]
#[path = "mcp_tests.rs"]
mod tests;

#[derive(Clone)]
pub struct McpConnectionManager {
    runtime: Arc<TalosMcpRuntime>,
    browser: Option<Arc<talos_browser::BrowserRuntime>>,
}

impl McpConnectionManager {
    pub fn from_runtime(runtime: Arc<TalosMcpRuntime>) -> Self {
        Self {
            runtime,
            browser: None,
        }
    }

    #[must_use]
    pub fn with_browser(mut self, browser: Arc<talos_browser::BrowserRuntime>) -> Self {
        self.browser = Some(browser);
        self
    }

    pub fn all_tool_specs(&self) -> Vec<RegisteredToolSpec> {
        let connections = self.runtime.connections();
        let mut specs = connections
            .iter()
            .flat_map(|connection| connection.tools().iter().map(tool_spec))
            .collect::<Vec<_>>();
        if connections.iter().any(McpConnection::supports_resources) {
            specs.extend(resource_tool_specs());
        }
        specs
    }

    pub(crate) fn resolve(
        self: &Arc<Self>,
        name: &str,
    ) -> Option<(RegisteredToolSpec, Arc<dyn crate::handlers::ToolHandler>)> {
        let (spec, connection) = if let Some((server, tool)) = split_mcp_tool_name(name) {
            let connection = self.runtime.connection(&server).ok()?;
            let spec = connection
                .tools()
                .iter()
                .find(|descriptor| descriptor.name == tool)
                .map(tool_spec)?;
            (spec, Some(connection))
        } else {
            if !self
                .runtime
                .connections()
                .iter()
                .any(McpConnection::supports_resources)
            {
                return None;
            }
            let spec = resource_tool_specs()
                .into_iter()
                .find(|spec| spec.definition.name == name)?;
            (spec, None)
        };
        Some((
            spec,
            Arc::new(crate::handlers::McpToolHandler::new(
                self.clone(),
                connection,
            )),
        ))
    }

    pub async fn call_tool(
        &self,
        connection: &McpConnection,
        tool: &str,
        arguments: Option<Value>,
        context: &ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let arguments = arguments.unwrap_or(Value::Null);
        let request = McpCallToolRequest {
            server_name: connection.name().to_string(),
            tool_name: tool.to_string(),
            arguments: arguments.clone(),
        };
        let response = if connection.is_builtin_browser() {
            let browser = self.browser.as_ref().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Browser execution is unavailable.",
                )
            })?;
            let required = |value: Option<&str>| {
                value
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        TalosError::new(
                            talos_error::ErrorCode::PermissionDenied,
                            "A trusted browser Turn is required.",
                        )
                    })
            };
            let action = talos_browser::BrowserAction::parse(tool, arguments)?;
            let lease = browser.authorize(
                talos_browser::BrowserInvocationIdentity {
                    user_id: context.user_id().to_owned(),
                    session_id: context.session_id().to_owned(),
                    turn_id: required(context.turn_id())?,
                    call_id: required(context.current_call_id())?,
                },
                &action,
            )?;
            if let talos_browser::BrowserAction::Upload { file_id, .. } = &action {
                let file = context.browser_upload(file_id).ok_or_else(|| {
                    TalosError::new(
                        talos_error::ErrorCode::PermissionDenied,
                        "Use a file attached to this Turn.",
                    )
                })?;
                browser.attach_upload(lease.token(), file)?;
            }
            let mut response = connection
                .call_builtin_browser_tool(request, lease.token(), cancelled(context))
                .await?;
            let downloads = browser.take_downloads(lease.token())?;
            if !downloads.is_empty() {
                let files = context
                    .image_store()?
                    .save_files(context.user_id(), downloads.clone())
                    .await?;
                browser.downloads_saved(lease.token(), downloads).await?;
                let payload = json!({"files":files});
                response.content.push(McpContent::Text {
                    text: payload.to_string(),
                });
                response.structured_content = Some(payload);
            }
            response
        } else {
            connection.call_tool(request, cancelled(context)).await?
        };
        call_tool_payload(response, context).await
    }

    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
        context: &ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let response = self
            .runtime
            .connection(server)?
            .read_resource(
                McpReadResourceRequest {
                    server_name: server.to_string(),
                    uri: uri.to_string(),
                },
                cancelled(context),
            )
            .await?;
        let content_items = response
            .contents
            .iter()
            .filter_map(|item| match item {
                McpResourceContents::Text { text, .. } => {
                    Some(TalosContentItem::Text { text: text.clone() })
                }
                McpResourceContents::Blob { .. } => None,
            })
            .collect();
        let structured = serde_json::to_value(response).map_err(|error| {
            TalosError::internal_with_source("Failed to encode the MCP resource result.", error)
        })?;
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            content_items,
        ))
    }
}

fn tool_spec(tool: &talos_types::McpToolDescriptor) -> RegisteredToolSpec {
    let name = mcp_tool_name(&tool.server_name, &tool.name);
    let description = tool.description.clone().unwrap_or_else(|| {
        format!(
            "MCP tool `{}` from server `{}`.",
            tool.name, tool.server_name
        )
    });
    RegisteredToolSpec::new(
        ToolDefinition::new(name.clone(), description, Some(tool.input_schema.clone())),
        ToolSource::Mcp,
        ToolPayloadKind::Mcp,
        ToolParallelism::SerialOnly,
        true,
        default_governance_for_source(ToolSource::Mcp),
    )
    .with_internal_name(name)
    .with_user_facing_name(format!("{}:{}", tool.server_name, tool.name))
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

async fn call_tool_payload(
    response: McpCallToolResponse,
    context: &ToolContext,
) -> TalosResult<ToolOutputPayload> {
    let files = images::persist_images(&response.content, context).await?;
    let mut payload = mcp_content_to_payload(&response.content, files)?;
    let structured = match response.structured_content {
        Some(value) => {
            payload.content_items.push(TalosContentItem::Json {
                value: JsonContentValue::from(value.clone()),
            });
            value
        }
        None => {
            let value = json!({ "content": payload.entries });
            if payload.content_items.is_empty() {
                payload.content_items.push(TalosContentItem::Json {
                    value: JsonContentValue::from(value.clone()),
                });
            }
            value
        }
    };
    let text = payload
        .content_items
        .iter()
        .filter_map(|item| match item {
            TalosContentItem::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(ToolOutputPayload::Mcp {
        content: if text.is_empty() && response.is_error {
            "The MCP tool reported a failure.".into()
        } else {
            text
        },
        is_error: response.is_error,
        structured_content: Some(structured),
        content_items: payload.content_items,
    })
}

struct McpPayloadContent {
    entries: Vec<Value>,
    content_items: Vec<TalosContentItem>,
}

fn mcp_content_to_payload(
    content: &[McpContent],
    files: Vec<talos_types::FileInfo>,
) -> TalosResult<McpPayloadContent> {
    let mut files = files.into_iter();
    let mut entries = Vec::new();
    let mut content_items = Vec::new();
    for item in content {
        match item {
            McpContent::Text { text } => {
                entries.push(json!({
                    "type": "text",
                    "text": text,
                }));
                content_items.push(TalosContentItem::Text { text: text.clone() });
            }
            McpContent::Image { .. } => {
                let file = files.next().ok_or_else(|| {
                    TalosError::internal("MCP image storage returned incomplete results.")
                })?;
                let reference = format!(
                    "To include this image in your reply, use ![Image](talos-file://{}). This reference requires the reader's File access.",
                    file.file_id
                );
                let item = TalosContentItem::Image {
                    file_id: file.file_id,
                    mime_type: file.mime_type,
                    label: file.original_filename,
                };
                entries.push(serde_json::to_value(&item).map_err(|error| {
                    TalosError::internal_with_source("Failed to encode MCP image reference.", error)
                })?);
                content_items.push(item);
                content_items.push(TalosContentItem::Text { text: reference });
            }
            McpContent::Resource { resource } => {
                let raw = serde_json::to_value(resource).map_err(|err| {
                    TalosError::new(
                        talos_error::ErrorCode::ToolExecutionFailed,
                        format!("serialize MCP resource content: {err}"),
                    )
                })?;
                entries.push(json!({
                    "type": "resource",
                    "resource": raw,
                }));
                content_items.push(TalosContentItem::Json {
                    value: JsonContentValue::from(raw),
                });
            }
        }
    }
    Ok(McpPayloadContent {
        entries,
        content_items,
    })
}

async fn cancelled(context: &ToolContext) {
    match context.cancellation() {
        Some(cancellation) => cancellation.requested().await,
        None => std::future::pending().await,
    }
}
