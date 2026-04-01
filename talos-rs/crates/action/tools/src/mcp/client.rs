use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::mcp::protocol::{
    CallToolParams, CallToolResult, InitializeParams, JsonRpcRequest, ListToolsResult,
    McpToolDefinition,
};
use crate::mcp::transport::McpTransport;

/// Single-server MCP client.
///
/// Wraps a [`McpTransport`] and provides typed methods for the MCP protocol.
pub struct McpClient {
    transport: Arc<dyn McpTransport>,
    next_id: AtomicU64,
}

impl McpClient {
    pub fn new(transport: Arc<dyn McpTransport>) -> Self {
        Self {
            transport,
            next_id: AtomicU64::new(1),
        }
    }

    /// Send the `initialize` handshake. Must be called before other methods.
    pub async fn initialize(&self) -> TalosResult<()> {
        let params = InitializeParams::default();
        let req = JsonRpcRequest::new(
            self.next_id(),
            "initialize",
            Some(serde_json::to_value(params).map_err(|e| {
                TalosError::ToolError(format!("MCP serialize initialize params: {e}"))
            })?),
        );
        let body = serde_json::to_string(&req)
            .map_err(|e| TalosError::ToolError(format!("MCP serialize request: {e}")))?;
        let response = self.transport.send(body).await?;
        if let Some(err) = response.error {
            return Err(TalosError::ToolError(format!(
                "MCP initialize error {}: {}",
                err.code, err.message
            )));
        }

        // Send `notifications/initialized` (fire-and-forget notification, no id).
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });
        let notif_body = serde_json::to_string(&notif)
            .map_err(|e| TalosError::ToolError(format!("MCP serialize notification: {e}")))?;
        // Best-effort; ignore transport errors for the notification.
        let _ = self.transport.send(notif_body).await;

        Ok(())
    }

    /// Retrieve the list of tools exposed by this server.
    pub async fn list_tools(&self) -> TalosResult<Vec<McpToolDefinition>> {
        let req = JsonRpcRequest::new(self.next_id(), "tools/list", None);
        let body = serde_json::to_string(&req)
            .map_err(|e| TalosError::ToolError(format!("MCP serialize request: {e}")))?;
        let response = self.transport.send(body).await?;
        if let Some(err) = response.error {
            return Err(TalosError::ToolError(format!(
                "MCP tools/list error {}: {}",
                err.code, err.message
            )));
        }
        let result: ListToolsResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| TalosError::ToolError("MCP tools/list: empty result".to_string()))?,
        )
        .map_err(|e| TalosError::ToolError(format!("MCP tools/list parse: {e}")))?;
        Ok(result.tools)
    }

    /// Invoke a tool by name with the given arguments.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> TalosResult<CallToolResult> {
        let params = CallToolParams {
            name: tool_name.to_string(),
            arguments,
        };
        let req = JsonRpcRequest::new(
            self.next_id(),
            "tools/call",
            Some(serde_json::to_value(params).map_err(|e| {
                TalosError::ToolError(format!("MCP serialize call params: {e}"))
            })?),
        );
        let body = serde_json::to_string(&req)
            .map_err(|e| TalosError::ToolError(format!("MCP serialize request: {e}")))?;
        let response = self.transport.send(body).await?;
        if let Some(err) = response.error {
            return Err(TalosError::ToolError(format!(
                "MCP tools/call error {}: {}",
                err.code, err.message
            )));
        }
        let result: CallToolResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| TalosError::ToolError("MCP tools/call: empty result".to_string()))?,
        )
        .map_err(|e| TalosError::ToolError(format!("MCP tools/call parse: {e}")))?;
        Ok(result)
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}
