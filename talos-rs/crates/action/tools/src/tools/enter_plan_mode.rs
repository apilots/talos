use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::state::PermissionMode;
use crate::{decode_tool_input, Tool, ToolContext, ToolOutputPayload};

pub struct EnterPlanModeTool;

#[derive(Debug, Deserialize)]
struct EnterPlanModeRequest {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct PlanModeResponse {
    mode: PermissionMode,
    reason: Option<String>,
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn internal_name(&self) -> &str {
        "enter_plan_mode"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["enter_plan_mode"]
    }

    fn description(&self) -> &str {
        "Request to enter plan mode for complex tasks that require exploration and design before coding."
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
                "reason": { "type": "string" }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: EnterPlanModeRequest = decode_tool_input(input, self.name())?;
        let mode = TaskStateRuntime::global().enter_plan_mode(context.session_id());
        ToolOutputPayload::success_json(
            &PlanModeResponse {
                mode,
                reason: request.reason,
            },
            "serialize enter_plan_mode",
        )
    }
}

#[cfg(test)]
#[path = "enter_plan_mode_tests.rs"]
mod tests;
