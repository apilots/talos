use super::*;
use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::state::PermissionMode;
use uuid::Uuid;

#[tokio::test]
async fn enter_plan_mode_sets_permission_mode() {
    let tool = EnterPlanModeTool;
    let session_id = format!("enter-plan-{}", Uuid::new_v4());

    let output = tool
        .execute_payload(
            serde_json::json!({ "reason": "Need explicit planning" }),
            ToolContext::new(&session_id, "user-1"),
        )
        .await
        .expect("enter_plan_mode should succeed");

    assert_eq!(
        TaskStateRuntime::global().current_permission_mode(&session_id),
        PermissionMode::Plan
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
                    "mode": "plan",
                    "reason": "Need explicit planning"
                }))
            );
        }
        other => panic!("expected function payload, got {other:?}"),
    }
}
