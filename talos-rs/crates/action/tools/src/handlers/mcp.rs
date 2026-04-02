use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use talos_error::{TalosError, TalosResult};

use crate::handlers::ToolHandler;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::mcp::McpConnectionManager;
use crate::output::{ToolDispatchOutcome, ToolOutputPayload};
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
        matches!(
            invocation.payload,
            ToolCallPayload::Mcp { .. } | ToolCallPayload::Function { .. }
        )
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
        _context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        match invocation.payload {
            ToolCallPayload::Mcp {
                server,
                tool,
                arguments,
            } => Ok(ToolDispatchOutcome::executed(
                self.manager
                    .call_tool(&server, &tool, Some(arguments))
                    .await?,
            )),
            ToolCallPayload::Function { arguments } => match invocation.tool_name.as_str() {
                "list_mcp_resources" => {
                    let args: ListArgs = serde_json::from_value(arguments).map_err(|err| {
                        TalosError::ToolError(format!(
                            "invalid list_mcp_resources arguments: {}",
                            err
                        ))
                    })?;
                    Ok(ToolDispatchOutcome::executed(
                        function_payload_from_mcp_result(
                            self.manager
                                .list_resources(args.server.as_deref(), args.cursor)
                                .await,
                        )?,
                    ))
                }
                "list_mcp_resource_templates" => {
                    let args: ListArgs = serde_json::from_value(arguments).map_err(|err| {
                        TalosError::ToolError(format!(
                            "invalid list_mcp_resource_templates arguments: {}",
                            err
                        ))
                    })?;
                    Ok(ToolDispatchOutcome::executed(
                        function_payload_from_mcp_result(
                            self.manager
                                .list_resource_templates(args.server.as_deref(), args.cursor)
                                .await,
                        )?,
                    ))
                }
                "read_mcp_resource" => {
                    let args: ReadResourceArgs =
                        serde_json::from_value(arguments).map_err(|err| {
                            TalosError::ToolError(format!(
                                "invalid read_mcp_resource arguments: {}",
                                err
                            ))
                        })?;
                    Ok(ToolDispatchOutcome::executed(
                        self.manager.read_resource(&args.server, &args.uri).await?,
                    ))
                }
                other => Err(TalosError::ToolError(format!(
                    "MCP tool '{}' requires Mcp payload",
                    other
                ))),
            },
            _ => Err(TalosError::ToolError(format!(
                "MCP tool '{}' invoked with unsupported payload",
                invocation.tool_name
            ))),
        }
    }
}

fn function_payload_from_mcp_result(
    result: TalosResult<serde_json::Value>,
) -> TalosResult<ToolOutputPayload> {
    match result {
        Ok(value) => Ok(ToolOutputPayload::Function {
            content: value.to_string(),
            success: true,
            structured_content: None,
        }),
        Err(TalosError::ToolError(message)) => {
            let mapped = map_mcp_json_rpc_error(message.as_str());
            Ok(ToolOutputPayload::Function {
                content: mapped.clone(),
                success: false,
                structured_content: Some(serde_json::json!({ "error": mapped })),
            })
        }
        Err(error) => Err(error),
    }
}

fn map_mcp_json_rpc_error(message: &str) -> String {
    if let Some((code, tail)) = parse_mcp_error_code(message) {
        return match code {
            -32601 => format!("MCP method not supported: {tail}"),
            -32602 => format!("Invalid MCP arguments: {tail}"),
            -32603 => format!("MCP server internal error: {tail}"),
            other => format!("MCP error {other}: {tail}"),
        };
    }
    message.to_string()
}

fn parse_mcp_error_code(message: &str) -> Option<(i64, &str)> {
    let marker = " error ";
    let index = message.find(marker)?;
    let rest = &message[index + marker.len()..];
    let (code, tail) = rest.split_once(':')?;
    let code = code.trim().parse::<i64>().ok()?;
    Some((code, tail.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_mcp_json_rpc_error_known_codes() {
        assert_eq!(
            map_mcp_json_rpc_error("MCP resources/list error -32601: missing"),
            "MCP method not supported: missing"
        );
        assert_eq!(
            map_mcp_json_rpc_error("MCP resources/list error -32602: bad args"),
            "Invalid MCP arguments: bad args"
        );
        assert_eq!(
            map_mcp_json_rpc_error("MCP resources/list error -32603: boom"),
            "MCP server internal error: boom"
        );
    }
}

#[derive(Debug, Deserialize)]
struct ListArgs {
    #[serde(default)]
    server: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReadResourceArgs {
    server: String,
    uri: String,
}
