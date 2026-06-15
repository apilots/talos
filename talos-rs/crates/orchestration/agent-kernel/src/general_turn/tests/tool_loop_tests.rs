use super::*;
use talos_llm_provider::ToolDefinition;
use talos_tools::{
    default_governance_for_source, RegisteredToolSpec, ToolParallelism, ToolPayloadKind, ToolSource,
};

#[derive(Default)]
struct DynamicLoopProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct DynamicResumeProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct PolicyStepProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct RecordingStepPolicy {
    before_step_count: AtomicUsize,
    after_step_count: AtomicUsize,
}

#[async_trait]
impl LlmProvider for DynamicLoopProvider {
    fn name(&self) -> &str {
        "dynamic-loop"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        match round {
            0 => Ok(ChatResponse {
                content: "Request dynamic tool result.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_dynamic",
                    "echo_test",
                    json!({ "message": "ping" }),
                )]),
            }),
            _ => panic!("dynamic flow should stop before a second model round"),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "dynamic-loop".to_string()
    }
}

#[async_trait]
impl LlmProvider for DynamicResumeProvider {
    fn name(&self) -> &str {
        "dynamic-resume"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        assert_eq!(round, 0, "resume should continue directly to model loop");
        let tool_message = request
            .messages
            .iter()
            .find(|message| {
                message.role == talos_llm_provider::Role::Tool
                    && message.tool_call_id.as_deref() == Some("call_dynamic")
                    && message.name.as_deref() == Some("echo_test")
            })
            .expect("dynamic resume should replay tool result before model step");
        assert!(tool_message.content.contains("dynamic pong"));
        Ok(ChatResponse {
            content: "Final answer after dynamic pong.".to_string(),
            reasoning: Vec::new(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "dynamic-resume".to_string()
    }
}

#[async_trait]
impl LlmProvider for PolicyStepProvider {
    fn name(&self) -> &str {
        "policy-step"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        assert_eq!(round, 0, "policy stop should end after one model step");
        assert_eq!(
            request.max_tokens,
            Some(77),
            "policy should be able to adjust the model step request"
        );
        Ok(ChatResponse {
            content: "provider text".to_string(),
            reasoning: Vec::new(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "policy-step".to_string()
    }
}

impl GeneralTurnPolicy for RecordingStepPolicy {
    fn before_step_request(&self, request: &mut GeneralTurnStepRequest) -> TalosResult<()> {
        self.before_step_count.fetch_add(1, Ordering::SeqCst);
        request.max_tokens = Some(77);
        Ok(())
    }

    fn after_step_result(
        &self,
        result: &mut GeneralTurnStepResult,
    ) -> TalosResult<Option<GeneralTurnPolicyStop>> {
        self.after_step_count.fetch_add(1, Ordering::SeqCst);
        result.text = "stopped by policy".to_string();
        Ok(Some(GeneralTurnPolicyStop {
            output_text: result.text.clone(),
            stop_reason: StopReason::EndTurn,
        }))
    }
}

#[tokio::test]

async fn test_run_general_turn_bash_then_task_output() {
    let provider = Arc::new(ExecLoopProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use Bash, then continue with TaskOutput.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert!(result.output_text.contains("done:ok"));
    assert!(result
        .output_items
        .iter()
        .any(|item| matches!(item, talos_types::talos::TalosOutputItem::ToolResult { .. })));
    assert!(provider.round.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn test_run_general_turn_stops_on_pending_dynamic_tool_result() {
    let tool_registry = Arc::new(ToolRegistry::new());
    let dynamic_spec = RegisteredToolSpec::new(
        ToolDefinition::new(
            "echo_test",
            "External dynamic echo tool",
            Some(json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            })),
        ),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    );
    tool_registry.register_external_dynamic_tool(dynamic_spec);

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let mut ctx = build_ctx_with_stream(None);

    let provider = Arc::new(DynamicLoopProvider::default());
    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use the external dynamic echo tool.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
    )
    .await
    .expect("general turn should succeed");

    assert!(result.output_text.is_empty());
    assert_eq!(result.stop_reason, StopReason::WaitingDynamicInput);
    assert!(matches!(
        result.outcome,
        GeneralTurnOutcome::Waiting {
            pending_action: PendingGeneralTurnAction::DynamicToolResult(_),
            stop_reason: StopReason::WaitingDynamicInput,
            ..
        }
    ));
    assert!(
        result.output_items.iter().all(|item| !matches!(
            item,
            talos_types::talos::TalosOutputItem::ToolResult { .. }
                | talos_types::talos::TalosOutputItem::OutputText { .. }
        )),
        "pending dynamic requests should not be emitted as terminal tool results"
    );
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_execute_general_turn_loop_uses_policy_step_hooks() {
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let provider = Arc::new(PolicyStepProvider::default());
    let policy = Arc::new(RecordingStepPolicy::default());
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "mock-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        prompt_prefix_messages: Vec::new(),
        messages: vec![ChatMessage::user("Stop from policy.")],
        tool_definitions: tool_registry.get_tool_definitions(),
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

    let result = execute_general_turn_loop_with_policy(
        prepared,
        "talos",
        provider.clone(),
        tool_registry,
        memory_store,
        &config,
        None,
        policy.clone(),
    )
    .await
    .expect("general turn policy should stop the loop");

    assert_eq!(result.output_text, "stopped by policy");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);
    assert_eq!(policy.before_step_count.load(Ordering::SeqCst), 1);
    assert_eq!(policy.after_step_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_execute_general_turn_loop_replays_dynamic_resume_before_model_step() {
    let tool_registry = Arc::new(ToolRegistry::new());
    let dynamic_spec = RegisteredToolSpec::new(
        ToolDefinition::new(
            "echo_test",
            "External dynamic echo tool",
            Some(json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            })),
        ),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    );
    tool_registry.register_external_dynamic_tool(dynamic_spec);

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    let provider = Arc::new(DynamicResumeProvider::default());
    let prepared = PreparedGeneralTurn {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        project_root: None,
        selected_model: "mock-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        prompt_prefix_messages: Vec::new(),
        messages: vec![ChatMessage::user(
            "Continue after external dynamic tool result.",
        )],
        tool_definitions: tool_registry.get_tool_definitions(),
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
    let resume = GeneralTurnResume::DynamicToolResult(DynamicToolResultResume {
        call_id: "call_dynamic".to_string(),
        tool_name: "echo_test".to_string(),
        output: "dynamic pong".to_string(),
        success: true,
        original_call: Some(ResumeToolCall {
            item_id: None,
            call_id: "call_dynamic".to_string(),
            tool_name: "echo_test".to_string(),
            arguments: json!({ "message": "ping" }).into(),
        }),
        replay_dynamic_tool_results: HashMap::new(),
    });

    let result = execute_general_turn_loop_with_resume(
        prepared,
        "talos",
        provider.clone(),
        tool_registry.clone(),
        memory_store,
        &config,
        Some(resume),
    )
    .await
    .expect("general turn resume should succeed");

    assert_eq!(result.output_text, "Final answer after dynamic pong.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert!(result.output_items.iter().any(|item| matches!(
        item,
        talos_types::talos::TalosOutputItem::ToolResult { call_id, tool_name, .. }
            if call_id == "call_dynamic" && tool_name == "echo_test"
    )));
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);
}
