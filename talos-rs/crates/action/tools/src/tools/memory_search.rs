use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::MemorySearchRequest;

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct MemorySearchTool;

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "MemorySearch"
    }

    fn internal_name(&self) -> &str {
        "memory_search"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["memory_search"]
    }

    fn description(&self) -> &str {
        "Search long-term memory and return bounded summaries with stable memory IDs."
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
            preview_lines: 40,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 20},
                "kinds": {
                    "type": "array",
                    "items": {"enum": ["preference", "fact", "decision", "task", "episode"]}
                },
                "sinceAt": {"type": "integer"},
                "untilAt": {"type": "integer"}
            }
        }))
    }

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if query.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "query is required".to_string(),
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
                "memory_search requires memory access".to_string(),
            )
        })?;
        let scope = context.memory_scope().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "memory_search requires a memory scope".to_string(),
            )
        })?;
        let request = serde_json::from_value::<MemorySearchRequest>(input).map_err(|err| {
            TalosError::new(talos_error::ErrorCode::InvalidRequest, err.to_string())
        })?;
        let response = reader.search(scope, request).await?;
        ToolOutputPayload::success_json(&response, "serialize memory_search result")
    }
}
