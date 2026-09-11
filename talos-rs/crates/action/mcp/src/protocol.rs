mod catalog;
pub(super) use catalog::{list_resource_templates, list_resources, list_tools};

use futures_util::StreamExt;
use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, ClientCapabilities, ClientInfo, ClientRequest,
    Implementation, ProtocolVersion, ReadResourceRequest, ReadResourceRequestParams, ServerResult,
};
use rmcp::service::{RunningService, ServiceError};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient};
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::filter_inherited_tool_environment;
use talos_types::{
    McpCallToolRequest, McpCallToolResponse, McpContent, McpReadResourceRequest,
    McpReadResourceResponse, McpResourceContents, McpServerDefinition, McpTransportDefinition,
};
use tokio::process::Command;
use tokio::time;

use crate::DEFAULT_TOOL_TIMEOUT;
pub(super) type RmcpService = RunningService<RoleClient, TalosMcpClientHandler>;

#[derive(Clone)]
pub(super) struct McpClient {
    pub(super) peer: rmcp::Peer<RoleClient>,
}
impl McpClient {
    pub(super) async fn call_tool(
        &self,
        request: McpCallToolRequest,
        cancellation: impl Future<Output = ()> + Send,
    ) -> TalosResult<McpCallToolResponse> {
        let service = &self.peer;
        let arguments = match request.arguments {
            Value::Object(map) => Some(map),
            Value::Null => None,
            other => {
                return Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("MCP tool arguments must be a JSON object, got {other}"),
                ));
            }
        };
        let mut params = CallToolRequestParams::new(request.tool_name);
        params.arguments = arguments;
        let response = request_response(
            service,
            ClientRequest::CallToolRequest(CallToolRequest::new(params)),
            cancellation,
            DEFAULT_TOOL_TIMEOUT,
        )
        .await?;
        match response {
            ServerResult::CallToolResult(result) => map_call_tool_result(result),
            ServerResult::InputRequiredResult(_) | ServerResult::CreateTaskResult(_) => {
                Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "This MCP operation requires an unsupported interactive or task capability.",
                ))
            }
            _ => Err(TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "MCP tools/call returned unexpected response".to_string(),
            )),
        }
    }

    pub(super) async fn read_resource(
        &self,
        request: McpReadResourceRequest,
        cancellation: impl Future<Output = ()> + Send,
    ) -> TalosResult<McpReadResourceResponse> {
        let service = &self.peer;
        let result = match request_response(
            service,
            ClientRequest::ReadResourceRequest(ReadResourceRequest::new(
                ReadResourceRequestParams::new(request.uri),
            )),
            cancellation,
            DEFAULT_TOOL_TIMEOUT,
        )
        .await?
        {
            ServerResult::ReadResourceResult(result) => result,
            ServerResult::InputRequiredResult(_) => {
                return Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "This MCP resource requires an unsupported interactive capability.",
                ));
            }
            _ => {
                return Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "MCP resources/read returned unexpected response",
                ));
            }
        };
        Ok(McpReadResourceResponse {
            contents: result
                .contents
                .into_iter()
                .map(map_resource_contents)
                .collect::<TalosResult<_>>()?,
        })
    }
}
pub(super) async fn request_response(
    peer: &rmcp::Peer<RoleClient>,
    request: ClientRequest,
    cancellation: impl Future<Output = ()> + Send,
    timeout: Duration,
) -> TalosResult<ServerResult> {
    tokio::pin!(cancellation);
    let deadline = time::Instant::now() + timeout;
    let mut handle = tokio::select! {
        biased;
        _ = &mut cancellation => return Err(TalosError::new(talos_error::ErrorCode::Cancelled, "The MCP request was cancelled before dispatch.")),
        handle = time::timeout_at(deadline, peer.send_request_with_option(request, rmcp::service::PeerRequestOptions::no_options())) => handle.map_err(|_| request_timeout())?.map_err(map_service_error)?,
    };
    let error = tokio::select! {
        response = &mut handle.rx => {
            let response = response.map_err(|_| map_service_error(ServiceError::TransportClosed))?.map_err(map_service_error)?;
            check_size(&response)?;
            return Ok(response);
        }
        _ = &mut cancellation => TalosError::new(talos_error::ErrorCode::Cancelled, "The MCP request was cancelled."),
        _ = time::sleep_until(deadline) => request_timeout(),
    };
    time::timeout(
        crate::CLEANUP_TIMEOUT,
        handle.cancel(Some("Talos request ended".into())),
    )
    .await
    .map_err(|_| {
        TalosError::new(
            talos_error::ErrorCode::RunCleanupUnconfirmed,
            "MCP cancellation did not finish within the cleanup deadline.",
        )
    })?
    .map_err(|error| {
        TalosError::new(
            talos_error::ErrorCode::RunCleanupUnconfirmed,
            "MCP cancellation could not be sent.",
        )
        .with_source(map_service_error(error))
    })?;
    Err(error)
}

fn request_timeout() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ToolExecutionFailed,
        "The MCP request timed out.",
    )
}

pub(super) const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_RECORDS: usize = 1_000;

/// Count encoded data without allocating another copy of an untrusted result.
fn check_size(value: &impl serde::Serialize) -> TalosResult<usize> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > MAX_RESPONSE_BYTES {
                return Err(std::io::Error::other("MCP response exceeds the byte limit"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value).map_err(|error| {
        TalosError::from_source(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The MCP response could not be represented within the byte limit.",
            error,
        )
    })?;
    Ok(count.0)
}

#[derive(Clone)]
pub(super) struct TalosMcpClientHandler {
    pub(super) tools_changed: Arc<tokio::sync::Notify>,
}

impl TalosMcpClientHandler {
    fn lifecycle() -> ClientLifecycleMode {
        ClientLifecycleMode::Auto {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            legacy_version: Some(ProtocolVersion::V_2025_11_25),
        }
    }

    fn new() -> Self {
        Self {
            tools_changed: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

impl ClientHandler for TalosMcpClientHandler {
    async fn on_tool_list_changed(&self, _: rmcp::service::NotificationContext<RoleClient>) {
        self.tools_changed.notify_one();
    }
    fn get_info(&self) -> ClientInfo {
        ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("talos-mcp", env!("CARGO_PKG_VERSION")),
        )
    }
}

pub(super) async fn connect_service(
    server: &McpServerDefinition,
    credentials: Option<&crate::McpConnectionCredentials>,
    cleanup: &Arc<crate::cleanup::Cleanup>,
    credential_failure: tokio::sync::oneshot::Sender<TalosError>,
) -> TalosResult<crate::session::McpSession> {
    let token = match credentials {
        Some(crate::McpConnectionCredentials::Token(values)) => Some(values),
        _ => None,
    };
    match &server.transport {
        McpTransportDefinition::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let mut env = env.clone();
            if let Some(secret_env) = token.and_then(|value| value.env.as_ref()) {
                env.extend(secret_env.clone());
            }
            let command = build_stdio_command(command, args, &env, cwd.as_deref());
            let (transport, mut process) = crate::process::McpProcess::spawn(command, cleanup)?;
            let result = tokio::select! {
                result = initialize_service(bounded_stdio(transport), cleanup) => result,
                status = process.wait() => Err(match status {
                    Ok(status) => TalosError::new(talos_error::ErrorCode::ToolExecutionFailed, "The MCP process exited before initialization completed.").context(format!("exit_status={status}")),
                    Err(error) => error,
                }),
            };
            match result {
                Ok(mut session) => {
                    session.process = Some(process);
                    Ok(session)
                }
                Err(error) => Err(error),
            }
        }
        McpTransportDefinition::StreamableHttp { url, headers } => {
            let mut config = rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url.clone())
                .max_sse_event_size(MAX_RESPONSE_BYTES).reinit_on_expired_session(false);
            let mut headers = headers.clone();
            if let Some(credentials) = token {
                if let Some(secret_headers) = &credentials.headers {
                    headers.extend(secret_headers.clone());
                }
                match (&credentials.authentication, &credentials.token) {
                    (talos_types::McpAuthentication::Bearer, Some(token)) => {
                        headers.insert("Authorization".into(), format!("Bearer {token}"));
                    }
                    (talos_types::McpAuthentication::Header { name }, Some(token)) => {
                        headers.insert(name.clone(), token.clone());
                    }
                    _ => {}
                }
            }
            for (name, value) in headers {
                let name = name.parse().map_err(|_| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "The MCP HTTP header name is invalid.",
                    )
                })?;
                let value = value.parse().map_err(|_| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "The MCP HTTP header value is invalid.",
                    )
                })?;
                if config.custom_headers.insert(name, value).is_some() {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "MCP HTTP header names must be unique regardless of case.",
                    ));
                }
            }
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Failed to construct the MCP HTTP client.",
                        error,
                    )
                })?;
            if let Some(crate::McpConnectionCredentials::OAuth(source)) = credentials {
                let client = crate::oauth::client(url, source, client, credential_failure).await?;
                return initialize_service(
                    StreamableHttpClientTransport::with_client(client, config),
                    cleanup,
                )
                .await;
            }
            initialize_service(
                StreamableHttpClientTransport::with_client(client, config),
                cleanup,
            )
            .await
        }
    }
}

/// Use the SDK codec's frame limit before JSON decoding. Its receive contract
/// exposes closure, not decode errors: a malformed or oversized frame terminates
/// this stream, and the connection owner reports the resulting transport failure.
fn bounded_stdio(
    (read, write): (tokio::process::ChildStdout, tokio::process::ChildStdin),
) -> impl rmcp::transport::Transport<rmcp::RoleClient> {
    use rmcp::transport::async_rw::JsonRpcMessageCodec;
    use tokio_util::codec::{FramedRead, FramedWrite};
    let read = FramedRead::new(
        read,
        JsonRpcMessageCodec::<rmcp::model::ServerJsonRpcMessage>::new_with_max_length(
            MAX_RESPONSE_BYTES,
        ),
    )
    .scan((), |_, message| std::future::ready(message.ok()));
    let write = FramedWrite::new(
        write,
        JsonRpcMessageCodec::<rmcp::model::ClientJsonRpcMessage>::default(),
    );
    rmcp::transport::sink_stream::SinkStreamTransport::new(write, read)
}

pub(super) async fn initialize_service<T, E, A>(
    transport: T,
    cleanup: &Arc<crate::cleanup::Cleanup>,
) -> TalosResult<crate::session::McpSession>
where
    T: rmcp::transport::IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let service = TalosMcpClientHandler::new()
        .serve_with_lifecycle(transport, TalosMcpClientHandler::lifecycle())
        .await
        .map_err(|error| {
            TalosError::from_source(
                talos_error::ErrorCode::ToolExecutionFailed,
                "The MCP server protocol negotiation failed.",
                error,
            )
        })?;
    let service = crate::session::McpSession::new(service, cleanup.clone());
    // rmcp validates discovery versions but accepts arbitrary legacy initialize responses.
    if !service
        .peer_info()
        .is_some_and(|info| ProtocolVersion::KNOWN_VERSIONS.contains(&info.protocol_version))
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The MCP server selected an unsupported protocol version.",
        ));
    }
    Ok(service)
}

fn map_call_tool_result(result: rmcp::model::CallToolResult) -> TalosResult<McpCallToolResponse> {
    Ok(McpCallToolResponse {
        content: result
            .content
            .into_iter()
            .map(map_content)
            .collect::<TalosResult<_>>()?,
        is_error: result.is_error.unwrap_or(false),
        structured_content: result.structured_content,
    })
}

pub(super) fn build_stdio_command(
    command: &str,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
    cwd: Option<&Path>,
) -> Command {
    let mut cmd = Command::new(command);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args)
        .env_clear()
        .envs(filter_inherited_tool_environment(std::env::vars()))
        .envs(env);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

fn map_content(content: rmcp::model::ContentBlock) -> TalosResult<McpContent> {
    match content {
        rmcp::model::ContentBlock::Text(text) => Ok(McpContent::Text { text: text.text }),
        rmcp::model::ContentBlock::Image(image) => Ok(McpContent::Image {
            data: image.data,
            mime_type: image.mime_type,
        }),
        rmcp::model::ContentBlock::Resource(resource) => Ok(McpContent::Resource {
            resource: map_resource_contents(resource.resource)?,
        }),
        _ => Err(TalosError::new(
            talos_error::ErrorCode::UnsupportedMediaType,
            "The MCP server returned an unsupported content type.",
        )),
    }
}

fn map_resource_contents(
    content: rmcp::model::ResourceContents,
) -> TalosResult<McpResourceContents> {
    match content {
        rmcp::model::ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            ..
        } => Ok(McpResourceContents::Text {
            uri,
            mime_type,
            text,
        }),
        rmcp::model::ResourceContents::BlobResourceContents {
            uri,
            mime_type,
            blob,
            ..
        } => Ok(McpResourceContents::Blob {
            uri,
            mime_type,
            blob,
        }),
        _ => Err(TalosError::new(
            talos_error::ErrorCode::UnsupportedMediaType,
            "The MCP server returned an unsupported resource type.",
        )),
    }
}

pub(super) fn map_service_error(mut error: ServiceError) -> TalosError {
    let message = match &mut error {
        ServiceError::McpError(remote) => {
            let reason = match remote.code.0 {
                -32601 => "The MCP server does not support this method.",
                -32602 => "The MCP server rejected the request arguments.",
                -32603 => "The MCP server reported an internal failure.",
                _ => "The MCP server rejected the request; remote details were withheld.",
            };
            remote.message = reason.into();
            remote.data = None;
            reason
        }
        ServiceError::TransportSend(_) => "Failed to send the request to the MCP server.",
        ServiceError::TransportClosed => "The MCP server connection closed.",
        ServiceError::UnexpectedResponse => "The MCP server returned an unexpected response type.",
        ServiceError::Cancelled { reason } => {
            *reason = None;
            "The MCP request was cancelled."
        }
        ServiceError::Timeout { .. } => "The MCP request timed out.",
        _ => {
            return TalosError::internal_with_source(
                "An unclassified MCP service failure occurred.",
                error,
            )
        }
    };
    TalosError::new(talos_error::ErrorCode::ToolExecutionFailed, message).with_source(error)
}
