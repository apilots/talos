use std::sync::Arc;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};

use crate::handlers::ToolHandler;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::mcp::McpConnectionManager;
use crate::output::ToolOutputPayload;
use crate::spec::ToolSource;

/// Dispatches MCP tool invocations to the appropriate server via [`McpConnectionManager`].
pub struct McpToolHandler {
    manager: Arc<McpConnectionManager>,
}

impl McpToolHandler {
    pub fn new(manager: Arc<McpConnectionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl ToolHandler for McpToolHandler {
    fn kind(&self) -> ToolSource {
        ToolSource::Mcp
    }

    fn matches(&self, invocation: &ToolInvocation) -> bool {
        matches!(invocation.payload, ToolCallPayload::Mcp { .. })
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
        _context: ToolDispatchContext,
    ) -> TalosResult<ToolOutputPayload> {
        let ToolCallPayload::Mcp {
            server,
            tool,
            arguments,
        } = invocation.payload
        else {
            return Err(TalosError::ToolError(format!(
                "MCP tool '{}' requires Mcp payload",
                invocation.tool_name
            )));
        };
        self.manager
            .call_tool(&server, &tool, Some(arguments))
            .await
    }
}
