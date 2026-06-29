use super::*;
use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::state::PermissionMode;
use crate::EnterPlanModeTool;
use uuid::Uuid;

#[tokio::test]
async fn exit_plan_mode_restores_default_permission_mode() {
    let enter_tool = EnterPlanModeTool;
    let exit_tool = ExitPlanModeTool;
    let session_id = format!("exit-plan-{}", Uuid::new_v4());

    enter_tool
        .execute_payload(
            serde_json::json!({}),
            ToolContext::new(&session_id, "user-1"),
        )
        .await
        .expect("enter_plan_mode should succeed");

    let output = exit_tool
        .execute_payload(
            serde_json::json!({ "summary": "Planning finished" }),
            ToolContext::new(&session_id, "user-1"),
        )
        .await
        .expect("exit_plan_mode should succeed");

    assert_eq!(
        TaskStateRuntime::global().current_permission_mode(&session_id),
        PermissionMode::Default
    );
    match output {
        ToolOutputPayload::Function {
            success,
            structured_content,
            ..
        } => {
            assert!(success);
            assert_eq!(
                structured_content,
                Some(serde_json::json!({
                    "mode": "default",
                    "summary": "Planning finished"
                }))
            );
        }
        other => panic!("expected function payload, got {other:?}"),
    }
}
