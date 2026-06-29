use super::*;
use crate::state::PermissionMode;

#[tokio::test]
async fn agent_rejects_empty_task() {
    let tool = AgentTool;
    let error = tool
        .execute_payload(
            serde_json::json!({ "prompt": "   ", "description": "Empty" }),
            ToolContext::new("agent-empty-task", "user-1"),
        )
        .await
        .expect_err("empty task should be rejected");

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[test]
fn build_agent_prompt_includes_extended_hints() {
    let prompt = build_agent_prompt(
        &AgentRequest {
            description: Some("Inspect bug".to_string()),
            prompt: Some("Review the failing flow".to_string()),
            task: None,
            subagent_type: Some("review".to_string()),
            model: Some("sonnet".to_string()),
            name: Some("Verifier".to_string()),
            team_name: Some("core".to_string()),
            mode: Some(PermissionMode::Plan),
            cwd: Some("/repo".to_string()),
            run_in_background: Some(false),
            execution_mode: None,
            wait_timeout_ms: None,
        },
        AgentKind::Review,
    );

    assert!(prompt.contains("Requested agent name:\nVerifier"));
    assert!(prompt.contains("Requested team name:\ncore"));
    assert!(prompt.contains("Requested permission mode:\nplan"));
    assert!(prompt.contains("Do not call the Agent tool again"));
}
