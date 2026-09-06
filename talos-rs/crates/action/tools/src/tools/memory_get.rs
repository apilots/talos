use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::MemoryGetRequest;

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct MemoryGetTool;

#[async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "MemoryGet"
    }

    fn internal_name(&self) -> &str {
        "memory_get"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["memory_get"]
    }

    fn description(&self) -> &str {
        "Get one complete long-term memory by the ID returned from MemorySearch."
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 32 * 1024,
            persist_large_output: false,
            preview_lines: 60,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["memoryId"],
            "properties": {
                "memoryId": {"type": "string"}
            }
        }))
    }

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        let memory_id = input
            .get("memoryId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if memory_id.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "memoryId is required".to_string(),
            ));
        }
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let reader = context.memory_reader().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "memory_get requires memory access".to_string(),
            )
        })?;
        let scope = context.memory_scope().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "memory_get requires a memory scope".to_string(),
            )
        })?;
        let request = serde_json::from_value::<MemoryGetRequest>(input).map_err(|err| {
            TalosError::new(talos_error::ErrorCode::InvalidRequest, err.to_string())
        })?;
        let response = reader.get(scope, request).await?;
        ToolOutputPayload::success_json(&response, "serialize memory_get result")
    }
}
