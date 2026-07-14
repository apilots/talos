use super::*;
use serde_json::json;

#[test]
fn test_invocation_context_state_write_scopes() {
    let mut ctx = InvocationContext {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "w1".to_string(),
        project_root: None,
        input: "hello".to_string(),
        model_selection: talos_types::ModelSelection::model_default(talos_types::ModelRef {
            provider: "p1".to_string(),
            id: "m1".to_string(),
        }),
        selected_model_info: ModelInfo::from_provider_model(
            "p1",
            talos_types::ProviderModelConfig {
                id: "m1".to_string(),
                name: "m1".to_string(),
                context_window: 0,
                reasoning: false,
                reasoning_effort: Default::default(),
                speed: Default::default(),
                input: Vec::new(),
                cost: Default::default(),
            },
        ),
        phase: TurnPhase::Execution,
        planning_instruction: None,
        transfer_to: None,
        execution_state: InvocationExecutionState::default(),
        session: crate::InvocationSession {
            session_id: "s1".to_string(),
            ..crate::InvocationSession::default()
        },
        session_environment: None,
        state: HashMap::new(),
        state_bags: ContextStateBags::default(),
        limits: InvocationLimits::default(),
        events: Vec::new(),
        transcript_service: None,
        stream_tx: None,
    };

    ctx.set_session_state("k1", json!("v1"));
    ctx.set_request_state("k2", json!("v2"));
    ctx.set_planning_response("ok");
    ctx.mark_transfer_consumed();

    assert_eq!(ctx.state_bags.session_state.get("k1"), Some(&json!("v1")));
    assert_eq!(ctx.state_bags.request_state.get("k2"), Some(&json!("v2")));
    assert_eq!(ctx.execution_state.planning_response.as_deref(), Some("ok"));
    assert!(ctx.transfer_consumed());
    assert!(ctx.set_user_state("k3", json!("v3")).is_err());
    assert!(ctx.set_workspace_state("k4", json!("v4")).is_err());
}
