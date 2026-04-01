use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};

use crate::mcp::protocol::JsonRpcResponse;
use crate::mcp::transport::McpTransport;

/// HTTP streamable MCP transport.
///
/// Sends each JSON-RPC request as a POST to the configured URL and reads the
/// response body as a single JSON object.
pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    bearer_token: Option<String>,
}

impl HttpTransport {
    pub fn new(url: impl Into<String>, bearer_token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: url.into(),
            bearer_token,
        }
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn send(&self, body: String) -> TalosResult<JsonRpcResponse> {
        let mut req = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .body(body);

        if let Some(token) = &self.bearer_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .send()
            .await
            .map_err(|e| TalosError::ToolError(format!("MCP HTTP transport error: {e}")))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| TalosError::ToolError(format!("MCP HTTP read error: {e}")))?;

        if !status.is_success() {
            return Err(TalosError::ToolError(format!(
                "MCP HTTP server returned {status}: {text}"
            )));
        }

        serde_json::from_str(&text).map_err(|e| {
            TalosError::ToolError(format!("MCP HTTP response parse error: {e}: {text}"))
        })
    }
}
