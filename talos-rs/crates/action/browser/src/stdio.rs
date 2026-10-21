//! Thin bundled MCP. Protocol negotiation and cancellation use the same SDK as other MCP connections.
use crate::BrowserActionExt;
use crate::{BrowserAction, INVOCATION_META_KEY, SOCKET_ENV};
use rmcp::{model::*, service::RequestContext, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{McpCallToolResponse, McpContent};

pub async fn run_stdio() -> TalosResult<()> {
    let tools = BrowserAction::catalog()
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Tool>, _>>()
        .map_err(|error| {
            TalosError::internal_with_source("The bundled Browser tool catalog is invalid.", error)
        })?;
    // The SDK codec bounds incoming frames before JSON decoding.
    let service = BrowserMcp {
        tools,
        slots: tokio::sync::Semaphore::new(4),
        socket: std::env::var_os(SOCKET_ENV).map(std::path::PathBuf::from),
    }
    .serve(rmcp::transport::stdio())
    .await
    .map_err(|error| {
        TalosError::internal_with_source("Browser MCP initialization failed.", error)
    })?;
    service.waiting().await.map_err(|error| {
        TalosError::internal_with_source("Browser MCP service ended unexpectedly.", error)
    })?;
    Ok(())
}
struct BrowserMcp {
    tools: Vec<Tool>,
    slots: tokio::sync::Semaphore,
    socket: Option<std::path::PathBuf>,
}
impl ServerHandler for BrowserMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("talos-browser", env!("CARGO_PKG_VERSION")),
        )
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.clone()))
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }
    async fn call_tool(
        &self,
        params: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let operation = async {
            let _slot = self.slots.try_acquire().map_err(|_| {
                TalosError::new(ErrorCode::Conflict, "Browser call capacity exceeded.")
            })?;
            let capability = context
                .meta
                .get(INVOCATION_META_KEY)
                .and_then(Value::as_str)
                .filter(|value| value.len() <= 128)
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::PermissionDenied,
                        "A trusted Worker invocation is required.",
                    )
                })?;
            let arguments = params.arguments.map_or_else(|| json!({}), Value::Object);
            BrowserAction::parse(&params.name, arguments.clone())?;
            #[cfg(unix)]
            {
                let path = self.socket.as_ref().ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Browser Worker bridge is not configured.",
                    )
                })?;
                crate::ipc::call(path, capability.into(), params.name.into_owned(), arguments).await
            }
            #[cfg(not(unix))]
            {
                let _ = (capability, arguments);
                Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Browser IPC is not supported on this platform.",
                ))
            }
        };
        let result = tokio::select! {
            _=context.ct.cancelled()=>Err(TalosError::new(ErrorCode::Cancelled,"Browser invocation was cancelled.")),
            result=tokio::time::timeout(std::time::Duration::from_secs(120),operation)=>
                result.unwrap_or_else(|error|Err(TalosError::from_source(ErrorCode::Timeout,"Browser invocation timed out.",error))),
        };
        let result = match result {
            Ok(value) => mcp_result(value),
            Err(error) => {
                json!({"isError":true,"content":[{"type":"text","text":error.to_api_error().message}],"structuredContent":{"error":error.to_api_error()}})
            }
        };
        serde_json::from_value::<CallToolResult>(result)
            .map(CallToolResponse::Complete)
            .map_err(|_| rmcp::ErrorData::internal_error("Browser MCP result is invalid.", None))
    }
}
fn mcp_result(response: McpCallToolResponse) -> Value {
    let content = response
        .content
        .into_iter()
        .map(|entry| match entry {
            McpContent::Text { text } => json!({"type":"text","text":text}),
            McpContent::Image { data, mime_type } => {
                json!({"type":"image","data":data,"mimeType":mime_type})
            }
            McpContent::Resource { resource } => json!({"type":"resource","resource":resource}),
        })
        .collect::<Vec<_>>();
    json!({"content":content,"isError":response.is_error,"structuredContent":response.structured_content})
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{BrowserInvocationIdentity, BrowserRuntime};
    use std::sync::Arc;
    #[tokio::test]
    async fn private_meta_round_trips_through_real_mcp_and_private_ipc_without_exposing_authority_in_arguments(
    ) {
        let home = tempfile::tempdir().unwrap();
        let runtime = BrowserRuntime::new(
            home.path().into(),
            talos_types::BrowserHostKind::Desktop,
            Arc::new(|| Ok(true)),
        )
        .unwrap();
        let backend = crate::tests::Backend::new();
        runtime.set_backend(backend.clone()).unwrap();
        let socket = runtime.start_ipc(home.path()).await.unwrap();
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let tools = BrowserAction::catalog()
            .into_iter()
            .map(|value| serde_json::from_value(value).unwrap())
            .collect();
        let server = BrowserMcp {
            tools,
            slots: tokio::sync::Semaphore::new(4),
            socket: Some(socket),
        };
        let serving =
            tokio::spawn(async move { server.serve(server_io).await.unwrap().waiting().await });
        let client = ().serve(client_io).await.unwrap();
        let catalog = client.list_all_tools().await.unwrap();
        assert_eq!(catalog.len(), 10);
        assert!(catalog
            .iter()
            .all(|tool| !tool.input_schema.contains_key(INVOCATION_META_KEY)));
        let params = CallToolRequestParams::new("browser_state");
        assert!(client
            .call_tool(params.clone())
            .await
            .unwrap()
            .is_error
            .unwrap_or(false));
        let lease = runtime
            .authorize(
                BrowserInvocationIdentity {
                    user_id: "alice".into(),
                    session_id: "s".into(),
                    turn_id: "t".into(),
                    call_id: "c".into(),
                },
                &BrowserAction::State,
            )
            .unwrap();
        let mut params = params;
        params.meta = Some(
            serde_json::Map::from_iter([(INVOCATION_META_KEY.into(), json!(lease.token()))]).into(),
        );
        backend.next.notify_one();
        let response = client.call_tool(params.clone()).await.unwrap();
        assert!(!response.is_error.unwrap_or(false), "{response:?}");
        assert!(client
            .call_tool(params)
            .await
            .unwrap()
            .is_error
            .unwrap_or(false));
        runtime.shutdown().await.unwrap();
        client.cancel().await.unwrap();
        serving.await.unwrap().unwrap();
    }
}
