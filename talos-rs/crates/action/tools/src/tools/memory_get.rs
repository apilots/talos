use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, MemoryGetResult};

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
        "Read a precise snippet from memory/*.md after MemorySearch returns a path."
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
            "required": ["path"],
            "properties": {
                "path": {"type": "string"},
                "from": {"type": "integer", "minimum": 1},
                "lines": {"type": "integer", "minimum": 1}
            }
        }))
    }

    fn validate_input(&self, input: &Value) -> TalosResult<()> {
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if path.is_empty() {
            return Err(TalosError::ValidationError("path is required".to_string()));
        }
        Ok(())
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let Some(memory) = context.memory_store() else {
            return Err(TalosError::ToolError(
                "memory_get requires an injected memory store in tool context".to_string(),
            ));
        };
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let from = input
            .get("from")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let lines = input
            .get("lines")
            .and_then(Value::as_u64)
            .map(|value| value as usize);

        let result: MemoryGetResult = memory.get_memory(&path, from, lines).await?;
        ToolOutputPayload::success_json(&result, "serialize memory_get result")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_config::AgentKernelConfig;

    fn test_kernel_config(workspace_dir: &std::path::Path) -> Arc<AgentKernelConfig> {
        let mut config = AgentKernelConfig::default();
        config.workspace_dir = workspace_dir.to_string_lossy().to_string();
        Arc::new(config)
    }

    #[tokio::test]
    async fn test_memory_get_errors_without_memory_store() {
        let workspace_dir = std::env::temp_dir().join(format!(
            "talos-memory-get-tool-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos()
        ));
        let tool = MemoryGetTool;
        let context = ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(test_kernel_config(&workspace_dir)),
        );
        let error = tool
            .execute_payload(
                serde_json::json!({ "path": "memory/2026-03-25.md" }),
                context,
            )
            .await
            .expect_err("tool should fail when memory store is missing");
        assert!(matches!(error, TalosError::ToolError(_)));
    }
}
