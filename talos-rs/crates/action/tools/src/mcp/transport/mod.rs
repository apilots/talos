pub mod http;
pub mod stdio;

pub use http::HttpTransport;
pub use stdio::StdioTransport;

use async_trait::async_trait;
use talos_error::TalosResult;

use crate::mcp::protocol::JsonRpcResponse;

/// Sends a single JSON-RPC request and returns the response.
#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn send(&self, body: String) -> TalosResult<JsonRpcResponse>;
}
