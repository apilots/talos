use super::*;
use crate::{
    BuiltInPlanner, ContextStateBags, InvocationExecutionState, InvocationLimits, TurnPhase,
};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, Role, Usage,
};
use talos_tools::{register_core_local_tools, ToolRegistry};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Default)]
struct RepeatedReadOnlyFlowProvider {
    round: AtomicUsize,
    forced_finalization_seen: AtomicBool,
    workspace_dir: String,
}

#[async_trait]
impl LlmProvider for RepeatedReadOnlyFlowProvider {
    fn name(&self) -> &str {
        "repeated-read-only-flow"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        if request.tools.as_ref().map_or(true, Vec::is_empty) {
            self.forced_finalization_seen.store(true, Ordering::SeqCst);
            assert!(
                request
                    .messages
                    .iter()
                    .any(|message| message.role == Role::Tool),
                "forced finalization should keep prior tool evidence in context"
            );
            return Ok(ChatResponse {
                content: "Final summary after repeated investigation.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            });
        }

        assert!(
            round < 3,
            "flow should inject a nudge on the second equivalent round and force finalization before a fourth tool-enabled round"
        );
        Ok(ChatResponse {
            content: "I should inspect the same directory again.".to_string(),
            reasoning: Vec::new(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("tool_calls".to_string()),
            tool_calls: Some(vec![FunctionCall::new(
                format!("call_{round}"),
                "Glob",
                json!({ "path": self.workspace_dir, "pattern": "**/*", "limit": 50 }),
            )]),
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }
}

fn build_ctx() -> InvocationContext {
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
        plugin_manager: crate::default_plugin_manager(),
        session_service: None,
        stream_tx: None,
    }
}

#[tokio::test]
async fn test_single_flow_forces_final_summary_after_repeated_read_only_round() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let provider = Arc::new(RepeatedReadOnlyFlowProvider {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    });
    let registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(registry.as_ref());
    let mut ctx = build_ctx();
    let flow = SingleFlow::new(
        "agent-1",
        "test agent",
        provider.clone(),
        registry.clone(),
        registry,
        Arc::new(MemoryStore::new(workspace_dir)),
        AgentKernelConfig::default(),
        Arc::new(BuiltInPlanner::new()),
    );

    let result = flow
        .run_events(&mut ctx, "Analyze the project structure.")
        .await
        .expect("flow should succeed");

    assert_eq!(
        result.summary.output_text,
        "Final summary after repeated investigation."
    );
    assert_eq!(result.summary.stop_reason, StopReason::ToolRoundLimit);
    assert_eq!(provider.round.load(Ordering::SeqCst), 4);
    assert!(provider.forced_finalization_seen.load(Ordering::SeqCst));
    assert_ne!(result.summary.output_text, "[tool-call rounds exceeded]");
}
