use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, ClientCapabilities, ClientInfo, ClientRequest,
    Implementation, PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams,
    ServerResult,
};
use rmcp::service::{RunningService, ServiceError};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientHandler, RoleClient};
use serde_json::Value;
use talos_error::{TalosError, TalosResult, TalosToolErrorKind};
use talos_types::{
    McpCallToolRequest, McpCallToolResponse, McpContent, McpReadResourceRequest,
    McpReadResourceResponse, McpResourceContents, McpResourceDescriptor,
    McpResourceTemplateDescriptor, McpServerDefinition, McpServerRuntimeDescriptor,
    McpServerSnapshot, McpServerStatus, McpToolDescriptor, McpTransportDefinition,
};
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio::time;

const DEFAULT_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TOOL_TIMEOUT: Duration = Duration::from_secs(120);

type RmcpService = RunningService<RoleClient, TalosMcpClientHandler>;

#[derive(Clone)]
pub struct TalosMcpRuntime {
    servers: Arc<RwLock<HashMap<String, ManagedMcpServer>>>,
}

impl TalosMcpRuntime {
    pub async fn connect_all(servers: Vec<McpServerRuntimeDescriptor>) -> Self {
        let runtime = Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
        };
        runtime.refresh(servers).await;
        runtime
    }

    pub async fn refresh(&self, servers: Vec<McpServerRuntimeDescriptor>) {
        let mut next = HashMap::new();
        for descriptor in servers {
            let name = descriptor.server.name.clone();
            let managed = match ManagedMcpServer::connect(descriptor.clone()).await {
                Ok(managed) => managed,
                Err(error) => ManagedMcpServer::failed(descriptor, error.to_string()),
            };
            next.insert(name, managed);
        }
        let previous = std::mem::replace(&mut *self.servers.write().await, next);
        for server in previous.into_values() {
            server.shutdown().await;
        }
    }

    pub async fn snapshots(&self) -> Vec<McpServerSnapshot> {
        let servers = self.servers.read().await;
        let mut snapshots = servers
            .values()
            .map(ManagedMcpServer::snapshot)
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.server.name.cmp(&right.server.name));
        snapshots
    }

    pub async fn tools(&self) -> Vec<McpToolDescriptor> {
        self.snapshots()
            .await
            .into_iter()
            .flat_map(|snapshot| snapshot.tools)
            .collect()
    }

    pub async fn call_tool(&self, request: McpCallToolRequest) -> TalosResult<McpCallToolResponse> {
        let servers = self.servers.read().await;
        let server = servers.get(&request.server_name).ok_or_else(|| {
            TalosError::ToolError(format!(
                "MCP server `{}` not connected",
                request.server_name
            ))
        })?;
        server.call_tool(request).await
    }

    pub async fn read_resource(
        &self,
        request: McpReadResourceRequest,
    ) -> TalosResult<McpReadResourceResponse> {
        let servers = self.servers.read().await;
        let server = servers.get(&request.server_name).ok_or_else(|| {
            TalosError::ToolError(format!(
                "MCP server `{}` not connected",
                request.server_name
            ))
        })?;
        server.read_resource(request).await
    }

    pub async fn shutdown(&self) {
        let previous = std::mem::take(&mut *self.servers.write().await);
        for server in previous.into_values() {
            server.shutdown().await;
        }
    }
}

enum ManagedMcpServer {
    Ready {
        descriptor: McpServerRuntimeDescriptor,
        service: Arc<RmcpService>,
        tools: Vec<McpToolDescriptor>,
        resources: Vec<McpResourceDescriptor>,
        resource_templates: Vec<McpResourceTemplateDescriptor>,
    },
    Failed {
        descriptor: McpServerRuntimeDescriptor,
        message: String,
    },
}

impl ManagedMcpServer {
    async fn connect(descriptor: McpServerRuntimeDescriptor) -> TalosResult<Self> {
        let service = Arc::new(connect_service(&descriptor.server).await?);
        let tools = list_tools(&descriptor.server.name, &service).await?;
        let supports_resources = service
            .peer()
            .peer_info()
            .is_some_and(|info| info.capabilities.resources.is_some());
        let (resources, resource_templates) = if supports_resources {
            (
                list_resources(&descriptor.server.name, &service).await?,
                list_resource_templates(&descriptor.server.name, &service).await?,
            )
        } else {
            (Vec::new(), Vec::new())
        };
        Ok(Self::Ready {
            descriptor,
            service,
            tools,
            resources,
            resource_templates,
        })
    }

    fn failed(descriptor: McpServerRuntimeDescriptor, message: String) -> Self {
        Self::Failed {
            descriptor,
            message,
        }
    }

    fn snapshot(&self) -> McpServerSnapshot {
        match self {
            Self::Ready {
                descriptor,
                tools,
                resources,
                resource_templates,
                ..
            } => McpServerSnapshot {
                server: descriptor.server.clone(),
                source: descriptor.source.clone(),
                status: McpServerStatus::Ready,
                tools: tools.clone(),
                resources: resources.clone(),
                resource_templates: resource_templates.clone(),
            },
            Self::Failed {
                descriptor,
                message,
            } => McpServerSnapshot {
                server: descriptor.server.clone(),
                source: descriptor.source.clone(),
                status: McpServerStatus::Failed {
                    message: message.clone(),
                },
                tools: Vec::new(),
                resources: Vec::new(),
                resource_templates: Vec::new(),
            },
        }
    }

    async fn call_tool(&self, request: McpCallToolRequest) -> TalosResult<McpCallToolResponse> {
        let Self::Ready { service, .. } = self else {
            return Err(TalosError::ToolError(format!(
                "MCP server `{}` is not ready",
                request.server_name
            )));
        };
        let arguments = match request.arguments {
            Value::Object(map) => Some(map),
            Value::Null => None,
            other => {
                return Err(TalosError::ToolError(format!(
                    "MCP tool arguments must be a JSON object, got {other}"
                )));
            }
        };
        let mut params = CallToolRequestParams::new(request.tool_name);
        params.arguments = arguments;
        let options = rmcp::service::PeerRequestOptions::no_options();
        let response = time::timeout(DEFAULT_TOOL_TIMEOUT, async {
            service
                .peer()
                .send_request_with_option(
                    ClientRequest::CallToolRequest(CallToolRequest::new(params)),
                    options,
                )
                .await?
                .await_response()
                .await
        })
        .await
        .map_err(|_| TalosError::ToolError("MCP tools/call timed out".to_string()))?
        .map_err(map_service_error)?;
        match response {
            ServerResult::CallToolResult(result) => Ok(map_call_tool_result(result)),
            _ => Err(TalosError::ToolError(
                "MCP tools/call returned unexpected response".to_string(),
            )),
        }
    }

    async fn read_resource(
        &self,
        request: McpReadResourceRequest,
    ) -> TalosResult<McpReadResourceResponse> {
        let Self::Ready { service, .. } = self else {
            return Err(TalosError::ToolError(format!(
                "MCP server `{}` is not ready",
                request.server_name
            )));
        };
        let params = ReadResourceRequestParams::new(request.uri);
        let result = time::timeout(DEFAULT_TOOL_TIMEOUT, service.read_resource(params))
            .await
            .map_err(|_| TalosError::ToolError("MCP resources/read timed out".to_string()))?
            .map_err(map_service_error)?;
        Ok(McpReadResourceResponse {
            contents: result
                .contents
                .into_iter()
                .map(map_resource_contents)
                .collect(),
        })
    }

    async fn shutdown(self) {
        if let Self::Ready { service, .. } = self {
            service.cancellation_token().cancel();
        }
    }
}

#[derive(Clone)]
struct TalosMcpClientHandler {
    client_info: ClientInfo,
}

impl TalosMcpClientHandler {
    fn new() -> Self {
        Self {
            client_info: ClientInfo::new(
                ClientCapabilities::default(),
                Implementation::new("talos-mcp", env!("CARGO_PKG_VERSION")),
            )
            .with_protocol_version(ProtocolVersion::V_2025_06_18),
        }
    }
}

impl ClientHandler for TalosMcpClientHandler {
    fn get_info(&self) -> ClientInfo {
        self.client_info.clone()
    }
}

async fn connect_service(server: &McpServerDefinition) -> TalosResult<RmcpService> {
    let handler = TalosMcpClientHandler::new();
    let service = match &server.transport {
        McpTransportDefinition::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let mut cmd = Command::new(command);
            cmd.kill_on_drop(true)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .args(args)
                .envs(env);
            if let Some(cwd) = cwd {
                cmd.current_dir(cwd);
            }
            let (transport, _stderr) = TokioChildProcess::builder(cmd).spawn().map_err(|err| {
                TalosError::ToolError(format!("failed to spawn MCP server `{command}`: {err}"))
            })?;
            time::timeout(
                DEFAULT_STARTUP_TIMEOUT,
                rmcp::service::serve_client(handler, transport),
            )
            .await
            .map_err(|_| {
                TalosError::ToolError(format!("MCP server `{}` startup timed out", server.name))
            })?
            .map_err(|err| TalosError::ToolError(format!("MCP initialize failed: {err}")))?
        }
        McpTransportDefinition::StreamableHttp { url, .. } => {
            let transport = StreamableHttpClientTransport::from_uri(url.clone());
            time::timeout(
                DEFAULT_STARTUP_TIMEOUT,
                rmcp::service::serve_client(handler, transport),
            )
            .await
            .map_err(|_| {
                TalosError::ToolError(format!("MCP server `{}` startup timed out", server.name))
            })?
            .map_err(|err| TalosError::ToolError(format!("MCP initialize failed: {err}")))?
        }
    };
    Ok(service)
}

async fn list_tools(
    server_name: &str,
    service: &RmcpService,
) -> TalosResult<Vec<McpToolDescriptor>> {
    let result = time::timeout(
        DEFAULT_STARTUP_TIMEOUT,
        service.list_tools(None::<PaginatedRequestParams>),
    )
    .await
    .map_err(|_| TalosError::ToolError("MCP tools/list timed out".to_string()))?
    .map_err(map_service_error)?;
    Ok(result
        .tools
        .into_iter()
        .map(|tool| McpToolDescriptor {
            server_name: server_name.to_string(),
            name: tool.name.to_string(),
            title: tool.title.map(|title| title.to_string()),
            description: tool.description.map(|description| description.to_string()),
            input_schema: serde_json::to_value(&*tool.input_schema).unwrap_or(Value::Null),
            annotations: tool
                .annotations
                .and_then(|annotations| serde_json::to_value(annotations).ok()),
        })
        .collect())
}

async fn list_resources(
    server_name: &str,
    service: &RmcpService,
) -> TalosResult<Vec<McpResourceDescriptor>> {
    let result = service
        .list_resources(None::<PaginatedRequestParams>)
        .await
        .map_err(map_service_error)?;
    Ok(result
        .resources
        .into_iter()
        .map(|resource| McpResourceDescriptor {
            server_name: server_name.to_string(),
            uri: resource.uri.clone(),
            name: Some(resource.name.clone()),
            description: resource.description.clone(),
            mime_type: resource.mime_type.clone(),
        })
        .collect())
}

async fn list_resource_templates(
    server_name: &str,
    service: &RmcpService,
) -> TalosResult<Vec<McpResourceTemplateDescriptor>> {
    let result = service
        .list_resource_templates(None::<PaginatedRequestParams>)
        .await
        .map_err(map_service_error)?;
    Ok(result
        .resource_templates
        .into_iter()
        .map(|resource| McpResourceTemplateDescriptor {
            server_name: server_name.to_string(),
            uri_template: resource.uri_template.clone(),
            name: Some(resource.name.clone()),
            description: resource.description.clone(),
            mime_type: resource.mime_type.clone(),
        })
        .collect())
}

fn map_call_tool_result(result: rmcp::model::CallToolResult) -> McpCallToolResponse {
    McpCallToolResponse {
        content: result.content.into_iter().map(map_content).collect(),
        is_error: result.is_error.unwrap_or(false),
        structured_content: result.structured_content,
    }
}

fn map_content(content: rmcp::model::Content) -> McpContent {
    match content.raw {
        rmcp::model::RawContent::Text(text) => McpContent::Text { text: text.text },
        rmcp::model::RawContent::Image(image) => McpContent::Image {
            data: image.data,
            mime_type: image.mime_type,
        },
        rmcp::model::RawContent::Resource(resource) => McpContent::Resource {
            resource: map_resource_contents(resource.resource),
        },
        raw => McpContent::Unknown {
            raw: serde_json::to_value(raw).unwrap_or(Value::Null),
        },
    }
}

fn map_resource_contents(content: rmcp::model::ResourceContents) -> McpResourceContents {
    match content {
        rmcp::model::ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            ..
        } => McpResourceContents::Text {
            uri,
            mime_type,
            text,
        },
        rmcp::model::ResourceContents::BlobResourceContents {
            uri,
            mime_type,
            blob,
            ..
        } => McpResourceContents::Blob {
            uri,
            mime_type,
            blob,
        },
    }
}

fn map_service_error(error: ServiceError) -> TalosError {
    TalosError::ToolFailure {
        kind: TalosToolErrorKind::RuntimeFailure,
        message: format!("MCP service error: {error}"),
        gate_source: None,
    }
}

pub fn mcp_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

pub fn split_mcp_tool_name(name: &str) -> Option<(String, String)> {
    let rest = name.strip_prefix("mcp__")?;
    let (server, tool) = rest.split_once("__")?;
    if server.is_empty() || tool.is_empty() {
        return None;
    }
    Some((server.to_string(), tool.to_string()))
}
