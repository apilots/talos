use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::bash::{read_task_output_with_context, TaskOutputRequest};
use crate::spec::{ToolBehaviorFlags, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutputPayload};

pub struct TaskOutputTool;

#[derive(Debug, Deserialize)]
struct TaskOutputInput {
    task_id: i64,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    max_output_tokens: Option<usize>,
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &str {
        "TaskOutput"
    }

    fn internal_name(&self) -> &str {
        "task_output"
    }

    fn description(&self) -> &str {
        "Read output from a background task by task ID.\n\nUsage:\n- Use this for background Bash tasks launched with run_in_background=true.\n- Set block semantics through repeated polling from the caller when needed.\n- Prefer reading the task output file directly once task lifecycle reporting is available."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": { "type": "integer" },
                "offset": { "type": "integer", "minimum": 0 },
                "max_output_tokens": { "type": "integer", "minimum": 1 }
            }
        }))
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: true,
            destructive: false,
            concurrency_safe: true,
            requires_user_interaction: false,
        }
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: TaskOutputInput = serde_json::from_value(input).map_err(|err| {
            TalosError::ToolError(format!("invalid TaskOutput arguments: {}", err))
        })?;
        let response = read_task_output_with_context(
            &TaskOutputRequest {
                session_id: input.task_id,
                chars: String::new(),
                yield_time_ms: input.offset.unwrap_or(250),
                max_output_tokens: input.max_output_tokens,
            },
            &context,
        )
        .await?;
        ToolOutputPayload::success_json(&response, "serialize TaskOutput response")
    }
}
