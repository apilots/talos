use super::*;
use serde_json::json;

#[test]
fn test_invocation_context_state_write_scopes() {
    let mut ctx = InvocationContext {
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "w1".to_string(),
        project_root: None,
        input: UserInput {
            text: "hello".to_string(),
            file_ids: Vec::new(),
        },
        resolved_attachments: Vec::new(),
        model_selection: talos_types::ModelSelection::without_reasoning(talos_types::ModelRef {
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
        session_artifact_store: None,
        session_store: None,
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

#[test]
fn kernel_execution_settings_follow_effective_permission_mode() {
    let mut config = talos_config::AgentKernelConfig {
        tool_parallelism: 4,
        ..talos_config::AgentKernelConfig::default()
    };

    config.tools.sandbox_policy.user_permission_mode =
        talos_config::ToolUserPermissionModeConfig::FullAccess;
    assert_eq!(
        KernelExecutionSettings::try_from(&config)
            .expect("full access settings")
            .tool_execution_mode,
        ToolExecutionMode::Parallel
    );

    config.tools.sandbox_policy.user_permission_mode =
        talos_config::ToolUserPermissionModeConfig::AskForApproval;
    assert_eq!(
        KernelExecutionSettings::try_from(&config)
            .expect("ask for approval settings")
            .tool_execution_mode,
        ToolExecutionMode::Sequential
    );

    config.tools.sandbox_policy.user_permission_mode =
        talos_config::ToolUserPermissionModeConfig::ApproveForMe;
    assert_eq!(
        KernelExecutionSettings::try_from(&config)
            .expect("approve for me settings")
            .tool_execution_mode,
        ToolExecutionMode::Sequential
    );
}

#[test]
fn full_access_respects_the_configured_parallelism_limit() {
    let mut config = talos_config::AgentKernelConfig {
        tool_parallelism: 1,
        ..talos_config::AgentKernelConfig::default()
    };
    config.tools.sandbox_policy.user_permission_mode =
        talos_config::ToolUserPermissionModeConfig::FullAccess;

    assert_eq!(
        KernelExecutionSettings::try_from(&config)
            .expect("sequential full access settings")
            .tool_execution_mode,
        ToolExecutionMode::Sequential
    );
}
