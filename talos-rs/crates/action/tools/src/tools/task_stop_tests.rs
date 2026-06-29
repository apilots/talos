use super::*;
use crate::runtimes::bash::{run_bash_command, BashCommandRequest, SandboxPermissions};

#[tokio::test]
async fn execute_payload_returns_cancelled_bash_output() {
    let context = ToolContext::new("task-stop-tool-session", "user")
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let exec = run_bash_command(
        &BashCommandRequest {
            cmd: "sleep 5".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 20,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should start");

    let task_id = exec.session_id.expect("session id");
    let payload = TaskStopTool
        .execute_payload(serde_json::json!({"task_id": task_id}), context)
        .await
        .expect("task stop should execute");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("task stop should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::BashOutput {
            status: TalosToolTaskStatus::Cancelled,
            session_id: Some(actual_task_id),
            ..
        }) if *actual_task_id == task_id
    ));
}
