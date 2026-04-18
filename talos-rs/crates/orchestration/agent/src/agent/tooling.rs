use super::AgentImpl;
use crate::types::AgentResponse;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, FunctionCall};

impl AgentImpl {
    #[allow(dead_code)]
    pub(crate) async fn process_tool_calls(
        &self,
        tool_calls: Vec<FunctionCall>,
    ) -> TalosResult<Vec<ChatMessage>> {
        let mut tool_messages = Vec::new();
        let context = self
            .tool_registry
            .build_context(
                format!("agent-{}", self.id),
                self.name.clone(),
                Some(Arc::new(self.kernel_config_snapshot())),
            )
            .with_memory_store(self.memory_store.clone());

        for tool_call in tool_calls {
            tracing::info!(
                "Agent {} executing tool call: {} with args: {:?}",
                self.id,
                tool_call.name,
                tool_call.arguments
            );

            let invocation = self.tool_registry.build_function_invocation(
                format!("agent-call-{}", tool_call.id),
                &tool_call.name,
                tool_call.arguments.clone(),
            )?;
            match self
                .tool_registry
                .execute_invocation_with_context(invocation, context.clone())
                .await
            {
                Ok(tool_result) => {
                    let content_json = match tool_result {
                        talos_tools::ToolDispatchOutcome::Executed { payload } => {
                            if payload.is_error() {
                                serde_json::json!({
                                    "ok": false,
                                    "tool": tool_call.name,
                                    "error": payload.error_message().unwrap_or_else(|| "unknown error".to_string()),
                                })
                            } else {
                                serde_json::json!({
                                    "ok": true,
                                    "tool": tool_call.name,
                                    "result": payload.content(),
                                })
                            }
                        }
                        talos_tools::ToolDispatchOutcome::Pending { pending, .. } => {
                            match pending {
                                talos_tools::ToolPending::Approval { prompt, .. } => {
                                    serde_json::json!({
                                        "ok": false,
                                        "tool": tool_call.name,
                                        "error": prompt,
                                    })
                                }
                                talos_tools::ToolPending::Dynamic { .. } => serde_json::json!({
                                    "ok": false,
                                    "tool": tool_call.name,
                                    "error": "dynamic tool response is pending",
                                }),
                            }
                        }
                    };

                    tool_messages.push(ChatMessage::tool(
                        tool_call.id.clone(),
                        tool_call.name.clone(),
                        content_json.to_string(),
                    ));
                }
                Err(e) => {
                    let err_json = serde_json::json!({
                        "ok": false,
                        "tool": tool_call.name,
                        "error": e.to_string(),
                    });

                    tool_messages.push(ChatMessage::tool(
                        tool_call.id.clone(),
                        tool_call.name.clone(),
                        err_json.to_string(),
                    ));
                }
            }
        }

        Ok(tool_messages)
    }

    #[allow(dead_code)]
    pub(crate) fn derive_bash_command(goal: &str) -> String {
        let trimmed = goal.trim();
        let normalized = trimmed.to_ascii_lowercase();
        for prefix in ["run ", "execute ", "exec "] {
            if normalized.starts_with(prefix) {
                let cmd = trimmed[prefix.len()..].trim();
                if !cmd.is_empty() {
                    return cmd.to_string();
                }
            }
        }
        trimmed.to_string()
    }

    pub(crate) async fn execute_tool_internal(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> TalosResult<AgentResponse> {
        tracing::info!(
            "Agent {} executing tool: {} with args: {:?}",
            self.id,
            tool_name,
            args
        );

        let tool_context = self
            .tool_registry
            .build_context(
                format!("agent-{}", self.id),
                self.name.clone(),
                Some(Arc::new(self.kernel_config_snapshot())),
            )
            .with_memory_store(self.memory_store.clone());
        let invocation = self.tool_registry.build_function_invocation(
            format!("agent-direct-{tool_name}"),
            tool_name,
            args,
        )?;
        let tool_result = self
            .tool_registry
            .execute_invocation_with_context(invocation, tool_context)
            .await?;

        let content = match tool_result {
            talos_tools::ToolDispatchOutcome::Executed { payload } => {
                if payload.is_error() {
                    format!(
                        "Tool execution failed: {}",
                        payload
                            .error_message()
                            .unwrap_or_else(|| "Unknown error".to_string())
                    )
                } else {
                    payload.content().to_string()
                }
            }
            talos_tools::ToolDispatchOutcome::Pending { pending, .. } => match pending {
                talos_tools::ToolPending::Approval { prompt, .. } => {
                    format!("Tool execution pending approval: {prompt}")
                }
                talos_tools::ToolPending::Dynamic { .. } => {
                    "Tool execution is waiting for a dynamic tool response".to_string()
                }
            },
        };

        Ok(AgentResponse {
            output_text: content,
            output_items: Vec::new(),
            reasoning: Vec::new(),
            tool_calls: vec![],
        })
    }
}
