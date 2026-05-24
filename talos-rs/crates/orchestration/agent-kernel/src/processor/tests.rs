use super::*;
use crate::{
    BuiltInPlanner, ContextStateBags, ExecutionBranch, InvocationLimits, PlannerKind,
    PlanningResponseEffect, TurnPhase,
};
use serde_json::json;
use std::collections::HashMap;

struct BranchingPlanner;

impl Planner for BranchingPlanner {
    fn mode(&self) -> crate::PlannerKind {
        PlannerKind::BuiltIn
    }

    fn process_response_effect(
        &self,
        _ctx: &InvocationContext,
        response: &LlmResponseEnvelope,
    ) -> PlanningResponseEffect {
        PlanningResponseEffect {
            normalized_text: Some(response.output_text.trim().to_string()),
            segments: Vec::new(),
            output_text_override: None,
            clear_output_text: false,
            state_delta: vec![crate::StateDelta {
                namespace: crate::StateNamespace::Temp,
                key: "plan_seen".to_string(),
                value: json!(true),
            }],
            branch_override: Some(ExecutionBranch::DirectAnswer),
        }
    }
}

fn mock_ctx() -> InvocationContext {
    InvocationContext {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "w1".to_string(),
        project_root: None,
        input: "hello".to_string(),
        selected_model_info: talos_types::ModelInfo {
            id: "m1".to_string(),
            name: "m1".to_string(),
            provider: "p1".to_string(),
            context_window: 0,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        },
        phase: TurnPhase::Execution,
        planning_instruction: None,
        transfer_to: None,
        execution_state: crate::InvocationExecutionState::default(),
        session: crate::InvocationSession {
            session_id: "s1".to_string(),
            ..crate::InvocationSession::default()
        },
        session_environment: None,
        state: HashMap::new(),
        state_bags: ContextStateBags::default(),
        limits: InvocationLimits::default(),
        events: Vec::new(),
        plugin_manager: crate::default_plugin_manager(),
        transcript_service: None,
        stream_tx: None,
    }
}

#[tokio::test]
async fn test_planning_response_processor_emits_typed_branch_override_event() {
    let planner: Arc<dyn Planner> = Arc::new(BranchingPlanner);
    let processor = PlanningResponseProcessor::new(planner);
    let mut ctx = mock_ctx();
    let mut response = LlmResponseEnvelope {
        output_text: "  answer ".to_string(),
        output_items: Vec::new(),
        branch: ExecutionBranch::ReactiveLoop,
        actions: EventActions::default(),
        allow_final_projection: true,
    };
    let output = processor.run(&mut ctx, &mut response).await.unwrap();
    let events = output.events;

    assert_eq!(
        ctx.execution_state.planning_response.as_deref(),
        Some("answer")
    );
    assert!(!ctx
        .state_bags
        .request_state
        .contains_key("execution_branch"));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].branch, ExecutionBranch::DirectAnswer);
    assert_eq!(
        events[0].actions.branch_override,
        Some(ExecutionBranch::DirectAnswer)
    );
    assert_eq!(events[0].actions.state_delta.len(), 1);
}

#[tokio::test]
async fn test_planning_request_processor_applies_built_in_native_thinking() {
    let planner: Arc<dyn Planner> = Arc::new(BuiltInPlanner::new());
    let processor = PlanningRequestProcessor::new(planner);
    let mut ctx = mock_ctx();
    let mut request = LlmRequestEnvelope {
        branch: ExecutionBranch::ModelThinking,
        selected_model_info: Some(talos_types::ModelInfo {
            id: "m1".to_string(),
            name: "m1".to_string(),
            provider: "p1".to_string(),
            context_window: 0,
            reasoning: true,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        }),
        ..LlmRequestEnvelope::default()
    };

    processor.run(&mut ctx, &mut request).await.unwrap();

    assert!(request.native_thinking.is_some());
    assert!(request.planning_instruction.is_none());
}

#[tokio::test]
async fn test_tool_context_processor_disables_tools_for_direct_answer() {
    let mut ctx = mock_ctx();
    let mut request = LlmRequestEnvelope {
        tool_definitions: vec![talos_llm_provider::ToolDefinition {
            name: "read".to_string(),
            description: "read file".to_string(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            })),
        }],
        parallel_tool_calls: true,
        contents_mode: ContentsMode::FullSession,
        branch: ExecutionBranch::DirectAnswer,
        ..LlmRequestEnvelope::default()
    };
    ToolContextRequestProcessor
        .run(&mut ctx, &mut request)
        .await
        .unwrap();

    assert!(request.tool_definitions.is_empty());
    assert!(!request.parallel_tool_calls);
}

#[tokio::test]
async fn test_agent_transfer_processor_emits_transfer_event_and_instruction() {
    let mut ctx = mock_ctx();
    ctx.transfer_to = Some("agent-b".to_string());
    let mut request = LlmRequestEnvelope {
        branch: ExecutionBranch::ReactiveLoop,
        ..LlmRequestEnvelope::default()
    };
    let output = AgentTransferRequestProcessor
        .run(&mut ctx, &mut request)
        .await
        .unwrap();

    let events = output.events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].actions.transfer_to.as_deref(), Some("agent-b"));
    assert!(request
        .prompt_prefix_messages
        .iter()
        .any(|message| message.content.contains("agent-b")));
}

#[tokio::test]
async fn test_compaction_processor_emits_typed_summary_event() {
    let mut ctx = mock_ctx();
    ctx.limits.context_compaction_max_bytes = 32;
    ctx.session.events = vec![
        crate::KernelTranscriptEvent {
            id: "evt-old-1".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("first user message that is long enough"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-1-assistant".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: crate::KernelTranscriptEventKind::AgentMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::assistant("first assistant reply that is long enough"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-2".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("second user message that is long enough"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-2-assistant".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: crate::KernelTranscriptEventKind::AgentMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::assistant("second assistant reply that is long enough"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-3".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-3".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("third user message that is long enough"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
    ];
    let mut request = LlmRequestEnvelope {
        branch: ExecutionBranch::ReactiveLoop,
        contents_mode: ContentsMode::FullSession,
        ..LlmRequestEnvelope::default()
    };

    let output = CompactionRequestProcessor
        .run(&mut ctx, &mut request)
        .await
        .unwrap();

    assert_eq!(output.events.len(), 1);
    let compaction = output.events[0]
        .actions
        .compaction
        .as_ref()
        .expect("expected compaction actions");
    assert_eq!(compaction.start_event_id, "evt-old-1");
    assert_eq!(compaction.end_event_id, "evt-old-2-assistant");
    match &output.events[0].payload {
        EventPayload::CompactionSummary { payload } => {
            assert!(payload.summary.contains("Compacted previous context"));
        }
        other => panic!("expected compaction summary payload, got {:?}", other),
    }
}

#[tokio::test]
async fn test_compaction_processor_compacts_whole_invocations_only() {
    let mut ctx = mock_ctx();
    ctx.limits.context_compaction_max_bytes = 40;
    ctx.session.events = vec![
        crate::KernelTranscriptEvent {
            id: "evt-old-1-user".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("first invocation user message that is long enough"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-1-agent".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: crate::KernelTranscriptEventKind::AgentMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::assistant(
                    "first invocation assistant message that is long enough",
                ),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-2-user".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("second invocation user message that is also long"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-2-agent".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-2".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: crate::KernelTranscriptEventKind::AgentMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::assistant("second invocation assistant message"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-old-3-user".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "old-3".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("third invocation user message"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
    ];
    let mut request = LlmRequestEnvelope {
        branch: ExecutionBranch::ReactiveLoop,
        contents_mode: ContentsMode::FullSession,
        ..LlmRequestEnvelope::default()
    };

    let output = CompactionRequestProcessor
        .run(&mut ctx, &mut request)
        .await
        .unwrap();

    let compaction = output.events[0]
        .actions
        .compaction
        .as_ref()
        .expect("expected compaction actions");
    assert_eq!(compaction.start_event_id, "evt-old-1-user");
    assert_eq!(compaction.end_event_id, "evt-old-2-agent");
}

#[tokio::test]
async fn test_compaction_processor_skips_current_invocation_events() {
    let mut ctx = mock_ctx();
    ctx.invocation_id = "current".to_string();
    ctx.limits.context_compaction_max_bytes = 32;
    ctx.session.events = vec![
        crate::KernelTranscriptEvent {
            id: "evt-history".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "history".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("short"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-current-1".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "current".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::User,
            kind: crate::KernelTranscriptEventKind::UserMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::user("current invocation message that is very long"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
        crate::KernelTranscriptEvent {
            id: "evt-current-2".to_string(),
            session_id: "s1".to_string(),
            invocation_id: "current".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: EventAuthor::Agent,
            kind: crate::KernelTranscriptEventKind::AgentMessage,
            payload: crate::KernelTranscriptPayload::Message {
                message: ChatMessage::assistant("current invocation reply that is very long"),
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp: chrono::Utc::now(),
        },
    ];
    let mut request = LlmRequestEnvelope {
        branch: ExecutionBranch::ReactiveLoop,
        contents_mode: ContentsMode::FullSession,
        ..LlmRequestEnvelope::default()
    };

    let output = CompactionRequestProcessor
        .run(&mut ctx, &mut request)
        .await
        .unwrap();

    assert!(output.events.is_empty());
}
