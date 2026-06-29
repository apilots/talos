use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosContentItem, TalosToolTaskStatus};

use crate::runtimes::bash::{stop_task_with_context, TaskStatus};
use crate::spec::{ToolBehaviorFlags, ToolVisibility};
use crate::{Tool, ToolContext, ToolOutputPayload};

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

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let input: TaskStopInput = serde_json::from_value(input)
            .map_err(|err| TalosError::ToolError(format!("invalid TaskStop arguments: {}", err)))?;
        let task_id = input.task_id;
        let response = stop_task_with_context(task_id, &context).await?;
        let structured = serde_json::to_value(&response)
            .map_err(|err| TalosError::ToolError(format!("serialize TaskStop response: {err}")))?;
        let status = match response.status {
            TaskStatus::Running => TalosToolTaskStatus::Running,
            TaskStatus::Stopped => TalosToolTaskStatus::Cancelled,
            TaskStatus::Exited => {
                if response.exit_code == Some(0) {
                    TalosToolTaskStatus::Completed
                } else {
                    TalosToolTaskStatus::Failed
                }
            }
        };
        Ok(ToolOutputPayload::success_with_content_items(
            structured,
            vec![TalosContentItem::BashOutput {
                session_id: response.session_id.or(Some(task_id)),
                status,
                exit_code: response.exit_code,
                stdout: response.stdout,
                stderr: response.stderr,
                truncated: response.truncated,
                elapsed_ms: Some(response.elapsed_ms),
            }],
        ))
    }
}

#[cfg(test)]
#[path = "task_stop_tests.rs"]
mod tests;
