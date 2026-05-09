use super::*;
use async_trait::async_trait;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, Role, Usage,
};
use talos_tools::{register_core_local_tools, ToolRegistry};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Default)]
struct BudgetFinalizationProvider {
    round: AtomicUsize,
    forced_finalization_seen: AtomicBool,
    workspace_dir: String,
}

#[async_trait]
impl LlmProvider for BudgetFinalizationProvider {
    fn name(&self) -> &str {
        "budget-finalization-provider"
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
                "forced finalization should retain tool results in context"
            );
            return Ok(ChatResponse {
                content: "Final summary after tool budget exhaustion.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            });
        }

        assert_eq!(
            round, 0,
            "tool budget should stop another tool-enabled round"
        );
        Ok(ChatResponse {
            content: "Inspect the directory once.".to_string(),
            reasoning: Vec::new(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("tool_calls".to_string()),
            tool_calls: Some(vec![FunctionCall::new(
                "call_0",
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

#[tokio::test]
async fn test_execute_general_turn_loop_forces_summary_when_budget_is_exhausted() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let provider = Arc::new(BudgetFinalizationProvider {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..Default::default()
    });
    let registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(registry.as_ref());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "mock-model".to_string(),
        max_output_tokens: None,
        prompt_prefix_messages: Vec::new(),
        messages: vec![ChatMessage::user("Inspect the project structure.")],
        tool_definitions: registry.get_tool_definitions(),
        prompt_diagnostics: PromptDiagnostics {
            mode: talos_prompt::PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
            window_usage: talos_prompt::PromptWindowUsage::default(),
            session_history_compaction: None,
        },
        warnings: Vec::new(),
        prompt_warning_state: None,
        stream_tx: None,
    };
    let mut config = AgentKernelConfig::default();
    config.max_tool_rounds = 1;

    let result = execute_general_turn_loop(
        prepared,
        "talos",
        provider.clone(),
        registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn loop should succeed");

    assert_eq!(
        result.output_text,
        "Final summary after tool budget exhaustion."
    );
    assert_eq!(result.stop_reason, StopReason::ToolRoundLimit);
    assert_eq!(provider.round.load(Ordering::SeqCst), 2);
    assert!(provider.forced_finalization_seen.load(Ordering::SeqCst));
    assert_ne!(result.output_text, "[tool-call rounds exceeded]");
}
