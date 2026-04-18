use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::bash::stop_task_with_context;
use crate::spec::{ToolBehaviorFlags, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutput};

pub struct TaskStopTool;

#[derive(Debug, Deserialize)]
struct TaskStopInput {
    task_id: i64,
}

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "TaskStop"
    }

    fn internal_name(&self) -> &str {
        "task_stop"
    }

    fn description(&self) -> &str {
        "Stop a running background task by task ID."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": { "type": "integer" }
            }
        }))
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: true,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let input: TaskStopInput = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid TaskStop arguments: {}", err)))?;
        stop_task_with_context(input.task_id, &context).await?;
        Ok(ToolOutput {
            success: true,
            output: serde_json::json!({ "stopped": true, "task_id": input.task_id }).to_string(),
            error: None,
        })
    }
}
