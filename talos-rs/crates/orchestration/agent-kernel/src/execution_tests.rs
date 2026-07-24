use super::*;
use crate::{ContextStateBags, InvocationExecutionState, InvocationLimits, TurnPhase, UserInput};
use std::collections::HashMap;

fn mock_ctx() -> InvocationContext {
    InvocationContext {
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
fn test_final_response_event_matches_adk_style_predicate() {
    let ctx = mock_ctx();
    let final_event = ExecutionEvent::final_response(&ctx, ExecutionBranch::ReactiveLoop, "done");
    let tool_event = ExecutionEvent::tool_call(
        &ctx,
        ExecutionBranch::ReactiveLoop,
        "call-1",
        "search",
        serde_json::json!({"q": "talos"}),
    );

    assert!(final_event.is_final_response());
    assert!(!tool_event.is_final_response());
    assert!(!final_event.id.is_empty());
}

#[tokio::test]
async fn test_execution_event_buffer_supports_branch_and_final_queries() {
    let ctx = mock_ctx();
    let mut buffer = ExecutionEventBuffer::new();
    buffer
        .push(ExecutionEvent::text(
            &ctx,
            EventAuthor::User,
            ExecutionBranch::ReactiveLoop,
            ExecutionEventKind::UserInput,
            "hello",
            false,
        ))
        .await
        .unwrap();
    buffer
        .push(ExecutionEvent::final_response(
            &ctx,
            ExecutionBranch::DirectAnswer,
            "done",
        ))
        .await
        .unwrap();

    assert_eq!(buffer.branch_events(ExecutionBranch::ReactiveLoop).len(), 1);
    assert!(buffer.has_final_response(ExecutionBranch::DirectAnswer));
    assert!(!buffer.has_final_response(ExecutionBranch::ReactiveLoop));
    assert!(buffer.last_non_partial_event().is_some());
}
