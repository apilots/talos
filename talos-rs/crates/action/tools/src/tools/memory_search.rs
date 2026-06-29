use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, MemorySearchQuery, MemorySearchResponse};

use crate::spec::{ToolBehaviorFlags, ToolResultPolicy};
use crate::{Tool, ToolContext, ToolOutputPayload};

fn resolve_search_defaults(context: &ToolContext) -> usize {
    context
        .kernel_config()
        .map(|config| config.memory.search_max_results)
        .unwrap_or(6)
        .max(1)
}

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
        "Search memory/*.md for prior work, decisions, dates, preferences, and todos. Use this before answering memory-dependent questions."
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
                "max_results": {"type": "integer", "minimum": 1}
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
            return Err(TalosError::ValidationError("query is required".to_string()));
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
                "memory_search requires an injected memory store in tool context".to_string(),
            ));
        };
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let default_max_results = resolve_search_defaults(&context);
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(default_max_results)
            .max(1);
        let results = memory
            .search_memory(&MemorySearchQuery { query, max_results })
            .await?;
        ToolOutputPayload::success_json(
            &MemorySearchResponse { results },
            "serialize memory_search result",
        )
    }
}

#[cfg(test)]
#[path = "memory_search_tests.rs"]
mod tests;
