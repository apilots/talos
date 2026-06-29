use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::state::PermissionMode;
use crate::{decode_tool_input, Tool, ToolContext, ToolOutputPayload};

pub struct ExitPlanModeTool;

#[derive(Debug, Deserialize)]
struct ExitPlanModeRequest {
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Serialize)]
struct PlanModeResponse {
    mode: PermissionMode,
    summary: Option<String>,
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn internal_name(&self) -> &str {
        "exit_plan_mode"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["exit_plan_mode"]
    }

    fn description(&self) -> &str {
        "Present the plan for approval and exit plan mode so implementation can begin."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: false,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 4 * 1024,
            persist_large_output: false,
            preview_lines: 10,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: ExitPlanModeRequest = decode_tool_input(input, self.name())?;
        let mode = TaskStateRuntime::global().exit_plan_mode(context.session_id());
        ToolOutputPayload::success_json(
            &PlanModeResponse {
                mode,
                summary: request.summary,
            },
            "serialize exit_plan_mode",
        )
    }
}

#[cfg(test)]
#[path = "exit_plan_mode_tests.rs"]
mod tests;
