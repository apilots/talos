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
    connection: Option<talos_mcp::McpConnection>,
}

impl McpToolHandler {
    pub fn new(
        manager: Arc<McpConnectionManager>,
        connection: Option<talos_mcp::McpConnection>,
    ) -> Self {
        Self {
            manager,
            connection,
        }
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
        context: ToolDispatchContext,
    ) -> TalosResult<ToolDispatchOutcome> {
        match invocation.payload {
            ToolCallPayload::Mcp {
                server,
                tool,
                arguments,
            } => {
                let connection = self.connection.as_ref().ok_or_else(|| {
                    TalosError::internal("MCP dispatch omitted its connection binding")
                })?;
                if server != connection.name()
                    || invocation.tool_name != talos_mcp::mcp_tool_name(&server, &tool)
                {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::ToolExecutionFailed,
                        "The MCP invocation does not match its authorized tool.",
                    ));
                }
                Ok(ToolDispatchOutcome::executed(
                    self.manager
                        .call_tool(connection, &tool, Some(arguments), context.tool_context())
                        .await?,
                ))
            }
            ToolCallPayload::Function { arguments } => match invocation.tool_name.as_str() {
                "ListMcpResources" => {
                    let args: ListArgs = serde_json::from_value(arguments).map_err(|err| {
                        TalosError::new(
                            talos_error::ErrorCode::ToolExecutionFailed,
                            format!("invalid ListMcpResources arguments: {}", err),
                        )
                    })?;
                    Ok(ToolDispatchOutcome::executed(
                        ToolOutputPayload::success_structured(
                            self.manager
                                .list_resources(
                                    args.server.as_deref(),
                                    args.cursor,
                                    context.tool_context(),
                                )
                                .await?,
                        ),
                    ))
                }
                "ListMcpResourceTemplates" => {
                    let args: ListArgs = serde_json::from_value(arguments).map_err(|err| {
                        TalosError::new(
                            talos_error::ErrorCode::ToolExecutionFailed,
                            format!("invalid ListMcpResourceTemplates arguments: {}", err),
                        )
                    })?;
                    Ok(ToolDispatchOutcome::executed(
                        ToolOutputPayload::success_structured(
                            self.manager
                                .list_resource_templates(
                                    args.server.as_deref(),
                                    args.cursor,
                                    context.tool_context(),
                                )
                                .await?,
                        ),
                    ))
                }
                "ReadMcpResource" => {
                    let args: ReadResourceArgs =
                        serde_json::from_value(arguments).map_err(|err| {
                            TalosError::new(
                                talos_error::ErrorCode::ToolExecutionFailed,
                                format!("invalid ReadMcpResource arguments: {}", err),
                            )
                        })?;
                    Ok(ToolDispatchOutcome::executed(
                        self.manager
                            .read_resource(&args.server, &args.uri, context.tool_context())
                            .await?,
                    ))
                }
                other => Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("MCP tool '{}' requires Mcp payload", other),
                )),
            },
            _ => Err(TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!(
                    "MCP tool '{}' invoked with unsupported payload",
                    invocation.tool_name
                ),
            )),
        }
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
