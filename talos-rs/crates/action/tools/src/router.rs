use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::invocation::{ToolCallPayload, ToolInvocation};
use crate::mcp::split_mcp_tool_name;
use crate::spec::ToolCatalog;

#[derive(Debug, Clone, Default)]
pub struct ToolRouter;

impl ToolRouter {
    /// Build an invocation from a raw tool name and arguments.
    ///
    /// MCP tools (names matching `mcp__<server>__<tool>`) are automatically
    /// routed to a `ToolCallPayload::Mcp` invocation; all others become
    /// `ToolCallPayload::Function`.
    pub fn build_function_invocation(
        &self,
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: Value,
        catalog: &ToolCatalog,
    ) -> TalosResult<ToolInvocation> {
        let call_id = call_id.into();
        let tool_name = tool_name.into();
        let invocation = if let Some((server, tool)) = split_mcp_tool_name(&tool_name) {
            ToolInvocation {
                call_id,
                tool_name,
                payload: ToolCallPayload::Mcp {
                    server,
                    tool,
                    arguments,
                },
            }
        } else {
            ToolInvocation::function(call_id, tool_name, arguments)
        };
        self.normalize(invocation, catalog)
    }

    pub fn normalize(
        &self,
        mut invocation: ToolInvocation,
        catalog: &ToolCatalog,
    ) -> TalosResult<ToolInvocation> {
        let spec = catalog.find(&invocation.tool_name).ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("Tool '{}' not found", invocation.tool_name),
            )
        })?;
        if invocation.payload.kind() != spec.payload_kind {
            return Err(TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!(
                    "Tool '{}' invoked with incompatible payload",
                    invocation.tool_name
                ),
            ));
        }
        invocation.tool_name = spec.definition.name.clone();
        Ok(invocation)
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
