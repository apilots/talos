use super::*;
use crate::{ContextStateBags, InvocationExecutionState, InvocationLimits, TurnPhase};
use std::collections::HashMap;

fn mock_ctx(input: &str) -> InvocationContext {
    InvocationContext {
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "w1".to_string(),
        project_root: None,
        input: input.to_string(),
        model_selection: talos_types::ModelSelection::without_reasoning(talos_types::ModelRef {
            provider: "p1".to_string(),
            id: "m1".to_string(),
        }),
        selected_model_info: talos_types::ModelInfo::from_provider_model(
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
    }
}

#[test]
fn test_built_in_planner_mode() {
    let planner = BuiltInPlanner::new();
    assert_eq!(planner.mode(), PlannerKind::BuiltIn);
}

#[test]
fn test_built_in_planner_adds_tool_guidance_only_when_tools_exist() {
    let planner = BuiltInPlanner::new();
    let ctx = mock_ctx("hello");
    let request = crate::LlmRequestEnvelope::default();
    assert!(planner
        .build_request_effect(&ctx, &request)
        .planning_instruction
        .is_none());

    let request_with_tools = crate::LlmRequestEnvelope {
        tool_definitions: vec![talos_llm_provider::ToolDefinition::new(
            "shell",
            "run shell",
            Some(serde_json::json!({"type":"object"})),
        )],
        ..crate::LlmRequestEnvelope::default()
    };
    let effect = planner.build_request_effect(&ctx, &request_with_tools);
    let instruction = effect
        .planning_instruction
        .expect("instruction should exist");
    assert!(instruction.contains("Use available tools"));
    assert!(instruction.contains("Do not answer from memory"));
    assert!(instruction.contains("honor that tool first"));
    assert!(instruction.contains("prefer Grep before Read"));
    assert!(instruction.contains("use Glob first only when path shape"));
    assert!(instruction.contains("instead of shell grep or rg"));
    assert!(instruction.contains("instead of cat or head"));
    assert!(instruction.contains("instead of ls or find"));
    assert!(instruction.contains("Use a direct answer only when no tool is needed"));
}
