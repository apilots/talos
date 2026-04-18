use super::AgentImpl;
use crate::types::AgentResponse;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, FunctionCall};
use talos_tools::{ToolOutputPayload, ToolResultEnvelope};

fn render_success_output(
    payload: Option<&ToolOutputPayload>,
    envelope: &ToolResultEnvelope,
) -> String {
    let structured = payload
        .and_then(|item| match item {
            ToolOutputPayload::Function {
                structured_content, ..
            } => structured_content.as_ref(),
            ToolOutputPayload::Resource { .. } | ToolOutputPayload::Mcp { .. } => None,
        })
        .or(envelope.structured_content.as_ref());

    if let Some(structured) = structured {
        if let Some(result) = structured.get("result").and_then(|value| value.as_str()) {
            return result.to_string();
        }

        if let Ok(text) = serde_json::to_string(structured) {
            return text;
        }
    }

    payload
        .map(|item| item.content().to_string())
        .filter(|content| !content.trim().is_empty())
        .unwrap_or_else(|| "tool execution completed".to_string())
}

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
                    let envelope = tool_result.envelope();
                    let content_json = match envelope.status {
                        talos_tools::ToolResultStatus::Success => serde_json::json!({
                            "ok": true,
                            "tool": tool_call.name,
                            "result": envelope
                                .structured_content
                                .clone()
                                .unwrap_or(serde_json::Value::Null),
                            "envelope": envelope,
                        }),
                        talos_tools::ToolResultStatus::Pending => serde_json::json!({
                            "ok": false,
                            "tool": tool_call.name,
                            "error": envelope
                                .pending
                                .as_ref()
                                .and_then(|pending| pending.prompt.clone())
                                .unwrap_or_else(|| "dynamic tool response is pending".to_string()),
                            "envelope": envelope,
                        }),
                        talos_tools::ToolResultStatus::Rejected
                        | talos_tools::ToolResultStatus::Failure => {
                            let next_turn_failure = envelope
                                .next_turn_failure_payload()
                                .and_then(|value| serde_json::to_value(value).ok());
                            let mut payload = serde_json::json!({
                                "ok": false,
                                "tool": tool_call.name,
                                "error": envelope
                                    .error
                                    .as_ref()
                                    .map(|item| item.message.clone())
                                    .unwrap_or_else(|| "unknown error".to_string()),
                                "envelope": envelope,
                            });
                            if let Some(next_turn_failure) = next_turn_failure {
                                if let Some(object) = payload.as_object_mut() {
                                    object
                                        .insert("next_turn_failure".to_string(), next_turn_failure);
                                }
                            }
                            payload
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
        let envelope = tool_result.envelope();
        let payload = tool_result.payload();

        let content = match envelope.status {
            talos_tools::ToolResultStatus::Success => render_success_output(payload, &envelope),
            talos_tools::ToolResultStatus::Pending => envelope
                .pending
                .as_ref()
                .and_then(|pending| pending.prompt.clone())
                .map(|prompt| format!("Tool execution pending approval: {prompt}"))
                .unwrap_or_else(|| {
                    "Tool execution is waiting for a dynamic tool response".to_string()
                }),
            talos_tools::ToolResultStatus::Rejected | talos_tools::ToolResultStatus::Failure => {
                format!(
                    "Tool execution failed: {}",
                    envelope
                        .error
                        .as_ref()
                        .map(|item| item.message.as_str())
                        .unwrap_or("Unknown error")
                )
            }
        };

        Ok(AgentResponse {
            output_text: content,
            output_items: Vec::new(),
            reasoning: Vec::new(),
            tool_calls: vec![],
        })
    }
}
