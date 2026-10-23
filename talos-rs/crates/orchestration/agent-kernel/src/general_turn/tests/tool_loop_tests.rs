use super::*;
use talos_llm_provider::ToolDefinition;
use talos_tools::{
    default_governance_for_source, handlers::dynamic::DynamicToolFn, RegisteredToolSpec, Tool,
    ToolContext, ToolOutputPayload, ToolParallelism, ToolPayloadKind, ToolSource,
};

#[derive(Default)]
struct DynamicLoopProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct DynamicResumeProvider {
    round: AtomicUsize,
}

#[derive(Clone, Copy)]
enum QuestionProviderMode {
    Initial,
    Resume,
}

struct QuestionProvider {
    mode: QuestionProviderMode,
    round: AtomicUsize,
}

#[derive(Default)]
struct ToolSearchActivationProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct PolicyStepProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct WorkingContextBudgetProvider {
    round: AtomicUsize,
}

#[derive(Default)]
struct RecordingStepPolicy {
    before_step_count: AtomicUsize,
    after_step_count: AtomicUsize,
}

struct LargeOutputTool;

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
                reasoning: ReasoningOutput {
                    display_text: vec!["prepare dynamic tool call".to_string()],
                    continuation: Some(ReasoningContinuation::Text(
                        "dynamic provider continuation".to_string(),
                    )),
                },
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
        let assistant_message = request
            .messages
            .iter()
            .find(|message| {
                message.role == talos_llm_provider::Role::Assistant
                    && message
                        .tool_calls
                        .as_ref()
                        .is_some_and(|calls| calls.iter().any(|call| call.id == "call_dynamic"))
            })
            .expect("dynamic resume should replay assistant tool call before its result");
        assert_eq!(
            assistant_message.reasoning_continuation,
            Some(ReasoningContinuation::Text(
                "dynamic provider continuation".to_string()
            ))
        );
        Ok(ChatResponse {
            content: "Final answer after dynamic pong.".to_string(),
            reasoning: ReasoningOutput::default(),
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
impl LlmProvider for QuestionProvider {
    fn name(&self) -> &str {
        "question"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        assert_eq!(self.round.fetch_add(1, Ordering::SeqCst), 0);
        match self.mode {
            QuestionProviderMode::Initial => Ok(ChatResponse {
                content: String::new(),
                reasoning: ReasoningOutput {
                    display_text: vec!["ask for execution mode".to_string()],
                    continuation: Some(ReasoningContinuation::Text(
                        "question continuation".to_string(),
                    )),
                },
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call-question",
                    "AskUserQuestion",
                    json!({
                        "header": "Execution Mode",
                        "question": "How should the agent proceed?",
                        "options": [
                            {"id": "plan", "label": "Plan", "description": "Think first"},
                            {"id": "direct", "label": "Direct", "description": "Act now"}
                        ]
                    }),
                )]),
            }),
            QuestionProviderMode::Resume => {
                let tool_messages = request
                    .messages
                    .iter()
                    .filter(|message| {
                        message.role == talos_llm_provider::Role::Tool
                            && message.name.as_deref() == Some("AskUserQuestion")
                    })
                    .collect::<Vec<_>>();
                assert_eq!(tool_messages.len(), 3);
                assert_eq!(
                    tool_messages
                        .iter()
                        .filter_map(|message| message.tool_call_id.as_deref())
                        .collect::<Vec<_>>(),
                    vec!["call-question", "call-question-2", "call-question-3"]
                );
                assert!(tool_messages.iter().all(|message| {
                    message.content.contains("selectedOption")
                        && message.content.contains("plan")
                        && message.content.contains("How should the agent proceed?")
                }));
                let assistant_message = request
                    .messages
                    .iter()
                    .find(|message| {
                        message.role == talos_llm_provider::Role::Assistant
                            && message.tool_calls.as_ref().is_some_and(|calls| {
                                calls.iter().any(|call| call.name == "AskUserQuestion")
                            })
                    })
                    .expect("question resume should preserve its assistant tool call");
                assert_eq!(
                    assistant_message
                        .tool_calls
                        .as_ref()
                        .expect("assistant tool calls")
                        .iter()
                        .map(|call| call.id.as_str())
                        .collect::<Vec<_>>(),
                    vec!["call-question", "call-question-2", "call-question-3"]
                );
                assert_eq!(
                    assistant_message.reasoning_continuation,
                    Some(ReasoningContinuation::Text(
                        "question continuation".to_string()
                    ))
                );
                Ok(ChatResponse {
                    content: "Proceeding with a plan.".to_string(),
                    reasoning: ReasoningOutput::default(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("stop".to_string()),
                    tool_calls: None,
                })
            }
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "question".to_string()
    }
}

#[async_trait]
impl LlmProvider for ToolSearchActivationProvider {
    fn name(&self) -> &str {
        "tool-search-activation"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        let tool_names = request
            .tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .map(|tool| tool.name.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        match round {
            0 => {
                assert!(tool_names.contains(&"ToolSearch".to_string()));
                assert!(!tool_names.contains(&"echo_test".to_string()));
                Ok(ChatResponse {
                    content: String::new(),
                    reasoning: ReasoningOutput {
                        display_text: vec!["find the requested tool".to_string()],
                        continuation: Some(ReasoningContinuation::Text(
                            "provider continuation".to_string(),
                        )),
                    },
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_search",
                        "ToolSearch",
                        json!({ "query": "echo_test" }),
                    )]),
                })
            }
            1 => {
                let tool_call_message = request
                    .messages
                    .iter()
                    .find(|message| {
                        message.role == talos_llm_provider::Role::Assistant
                            && message.tool_calls.as_ref().is_some_and(|calls| {
                                calls.iter().any(|call| call.id == "call_search")
                            })
                    })
                    .expect("assistant tool-call message should be replayed");
                assert_eq!(
                    tool_call_message.reasoning_continuation,
                    Some(ReasoningContinuation::Text(
                        "provider continuation".to_string()
                    ))
                );
                assert!(tool_names.contains(&"echo_test".to_string()));
                Ok(ChatResponse {
                    content: String::new(),
                    reasoning: ReasoningOutput::default(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_echo",
                        "echo_test",
                        json!({ "message": "ping" }),
                    )]),
                })
            }
            2 => {
                let tool_message = request
                    .messages
                    .iter()
                    .find(|message| {
                        message.role == talos_llm_provider::Role::Tool
                            && message.tool_call_id.as_deref() == Some("call_echo")
                            && message.name.as_deref() == Some("echo_test")
                    })
                    .expect("activated dynamic tool result should be in the next model request");
                assert!(tool_message.content.contains("dynamic pong"));
                Ok(ChatResponse {
                    content: "activated done".to_string(),
                    reasoning: ReasoningOutput::default(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("stop".to_string()),
                    tool_calls: None,
                })
            }
            other => panic!("unexpected model round {other}"),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "tool-search-activation".to_string()
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
            reasoning: ReasoningOutput::default(),
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

#[async_trait]
impl Tool for LargeOutputTool {
    fn name(&self) -> &str {
        "LargeOutput"
    }

    fn description(&self) -> &str {
        "Return a large deterministic payload for working context budget tests"
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text(
            "large-output ".repeat(1_000),
        ))
    }
}

#[async_trait]
impl LlmProvider for WorkingContextBudgetProvider {
    fn name(&self) -> &str {
        "working-context-budget"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        match round {
            0 => Ok(ChatResponse {
                content: "Call the large output tool.".to_string(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(
                    (0..3)
                        .map(|index| {
                            FunctionCall::new(
                                format!("call-large-output-{index}"),
                                "LargeOutput",
                                json!({}),
                            )
                        })
                        .collect(),
                ),
            }),
            _ => panic!("working context budget should fail before another provider request"),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "working-context-budget".to_string()
    }
}

#[tokio::test]
async fn test_run_general_turn_bash_completes_directly() {
    let provider = Arc::new(ExecLoopProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let mut ctx = build_ctx_with_stream_for_session("tool-loop-bash-direct", None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use Bash and continue from its completed result.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store.clone(),
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("general turn should succeed");

    assert!(result.output_text().contains("done:ok"));
    assert_eq!(result.metrics().tool_calls, 1);
    assert_eq!(result.metrics().tool_successes, 1);
    assert_eq!(result.metrics().tool_errors, 0);
    assert_eq!(result.usage().total_tokens, 180);
    assert_eq!(
        result.last_model_usage,
        TokenUsage {
            input_tokens: 90,
            output_tokens: 20,
            total_tokens: 110,
            ..TokenUsage::default()
        }
    );
    assert!(result
        .output_items()
        .iter()
        .any(|item| matches!(item, KernelRunItem::ToolResult { .. })));
    assert_eq!(provider.round.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_run_general_turn_fails_before_provider_when_working_context_remains_too_large() {
    let provider = Arc::new(WorkingContextBudgetProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    tool_registry.register(Arc::new(LargeOutputTool));

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.prompt.window.compaction_buffer_tokens = 8;
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 32 };
    config.compaction.tool_output_keep_last = 1;
    configure_test_skill_root(&mut config, &workspace_dir);
    let mut ctx = build_ctx_with_stream_for_session("working-context-budget", None);
    // The initial prompt fits. Several individually bounded tool results still
    // exceed the window while their latest complete exchange must be retained.
    ctx.selected_model_info.model.context_window = 2_048;

    let error = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Call the large output tool and continue.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store.clone(),
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect_err("oversized working context should fail before the next provider request");

    assert!(
        error
            .to_string()
            .contains("working context budget remains exhausted before model request"),
        "{error}"
    );
    assert_eq!(
        provider.round.load(Ordering::SeqCst),
        1,
        "provider should only receive the initial tool-call request: {error}"
    );
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
    let memory_store = Arc::new(TestMemoryStore::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let mut ctx = build_ctx_with_stream_for_session("tool-loop-pending-dynamic", None);

    let provider = Arc::new(DynamicLoopProvider::default());
    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use the external dynamic echo tool.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store.clone(),
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("general turn should succeed");

    assert!(result.output_text().is_empty());
    assert_eq!(result.stop_reason(), StopReason::WaitingUserAction);
    assert_eq!(result.metrics().tool_calls, 1);
    assert_eq!(result.metrics().tool_successes, 0);
    assert_eq!(result.metrics().tool_errors, 0);
    assert_eq!(result.metrics().model_rounds, 1);
    let GeneralTurnOutcome::Waiting {
        ref pending_action, ..
    } = result.outcome
    else {
        panic!("dynamic tool result should remain pending");
    };
    assert_eq!(
        pending_action.reasoning_continuation,
        Some(ReasoningContinuation::Text(
            "dynamic provider continuation".to_string()
        ))
    );
    assert!(
        result.output_items().iter().all(|item| !matches!(
            item,
            KernelRunItem::ToolResult { .. } | KernelRunItem::OutputText { .. }
        )),
        "pending dynamic requests should not be emitted as terminal tool results"
    );
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_run_general_turn_activates_external_tool_with_tool_search_in_same_turn() {
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
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
    let dynamic_fn: DynamicToolFn = Arc::new(|_args, _ctx| {
        Box::pin(async move { Ok(ToolOutputPayload::success_text("dynamic pong")) })
    });
    tool_registry.register_dynamic_tool(dynamic_spec, dynamic_fn);

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let mut ctx = build_ctx_with_stream_for_session("tool-loop-tool-search-activation", None);

    let provider = Arc::new(ToolSearchActivationProvider::default());
    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Find and use the external echo tool.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(result.output_text(), "activated done");
    assert_eq!(provider.round.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_execute_general_turn_loop_uses_policy_step_hooks() {
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let provider = Arc::new(PolicyStepProvider::default());
    let policy = Arc::new(RecordingStepPolicy::default());
    let prepared = PreparedGeneralTurn {
        browser_uploads: Vec::new(),
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "default".to_string(),
        project_root: None,
        selected_model: "mock-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        attachment_tokens: 0,
        image_input_enabled: false,
        messages: vec![ChatMessage::user("Stop from policy.")],
        maintenance_usage: TokenUsage::default(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default()
            .with_session_id("s-policy"),
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
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
        run_tasks: crate::test_run_tasks(),
    };

    let result = execute_general_turn_loop_with_policy(
        prepared,
        provider.clone(),
        tool_registry,
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
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let provider = Arc::new(DynamicResumeProvider::default());
    let prepared = PreparedGeneralTurn {
        browser_uploads: Vec::new(),
        session_id: "s1".to_string(),
        user_id: "u1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "default".to_string(),
        project_root: None,
        selected_model: "mock-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        attachment_tokens: 0,
        image_input_enabled: false,
        messages: vec![ChatMessage::user(
            "Continue after external dynamic tool result.",
        )],
        maintenance_usage: TokenUsage::default(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default()
            .with_session_id("s-dynamic-resume"),
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
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
        run_tasks: crate::test_run_tasks(),
    };
    let resume = AgentKernelContinuation::UserAction(AgentKernelUserActionContinuation {
        call_id: "call_dynamic".to_string(),
        tool_name: "echo_test".to_string(),
        response: talos_types::UserActionResponse::DynamicToolResult(
            talos_types::DynamicToolResult {
                output: "dynamic pong".to_string(),
                success: true,
            },
        ),
        sandbox_request_fingerprint: None,
        original_call: Some(AgentKernelContinuationToolCall {
            item_id: None,
            call_id: "call_dynamic".to_string(),
            tool_name: "echo_test".to_string(),
            arguments: json!({ "message": "ping" }).into(),
            reasoning_continuation: Some(ReasoningContinuation::Text(
                "dynamic provider continuation".to_string(),
            )),
        }),
        tool_calls: vec![FunctionCall::new(
            "call_dynamic",
            "echo_test",
            json!({ "message": "ping" }),
        )],
        tool_messages: Vec::new(),
        deferred_tool_calls: Vec::new(),
        replay_dynamic_tool_results: HashMap::new(),
        execution_metrics: TurnExecutionMetrics {
            model_rounds: 1,
            tool_calls: 1,
            stop_reason: talos_types::talos::StopReason::WaitingUserAction,
            ..TurnExecutionMetrics::default()
        },
    });

    let result = execute_general_turn_loop_with_resume(
        prepared,
        provider.clone(),
        tool_registry.clone(),
        &config,
        Some(resume),
    )
    .await
    .expect("general turn resume should succeed");

    assert_eq!(result.output_text, "Final answer after dynamic pong.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert_eq!(result.metrics.tool_calls, 1);
    assert_eq!(result.metrics.tool_successes, 1);
    assert_eq!(result.metrics.tool_errors, 0);
    assert_eq!(result.metrics.model_rounds, 2);
    assert!(result.output_items.iter().any(|item| matches!(
        item,
        KernelRunItem::ToolResult { call_id, tool_name, .. }
            if call_id == "call_dynamic" && tool_name == "echo_test"
    )));
    assert_eq!(provider.round.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn question_pending_and_resume_continue_the_original_tool_call() {
    let tool_registry = Arc::new(ToolRegistry::new());
    talos_tools::register_core_local_tools(tool_registry.as_ref());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(TestMemoryStore::new());
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let mut ctx = build_ctx_with_stream_for_session("question-pending", None);

    let initial = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Ask a question before proceeding.",
        Arc::new(QuestionProvider {
            mode: QuestionProviderMode::Initial,
            round: AtomicUsize::new(0),
        }),
        tool_registry.clone(),
        tool_registry.clone(),
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("question should suspend the turn");
    let mut initial_metrics = initial.metrics().clone();
    let GeneralTurnOutcome::Waiting { pending_action, .. } = initial.outcome else {
        panic!("question should produce one pending user action");
    };
    let mut pending = *pending_action;
    assert!(matches!(
        pending.params,
        talos_types::UserActionParams::Question(ref params)
            if params.options.len() == 2 && params.question == "How should the agent proceed?"
    ));

    let prepared = PreparedGeneralTurn {
        browser_uploads: Vec::new(),
        session_id: "s-question".to_string(),
        user_id: "u1".to_string(),
        invocation_id: "turn-question".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "default".to_string(),
        project_root: None,
        selected_model: "mock-model".to_string(),
        max_output_tokens: None,
        reasoning_effort: None,
        speed: Default::default(),
        attachment_tokens: 0,
        image_input_enabled: false,
        messages: vec![ChatMessage::user("Continue after the answer.")],
        maintenance_usage: TokenUsage::default(),
        tool_selection_request: talos_tools::ToolSelectionRequest::default()
            .with_session_id("s-question"),
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
        prompt_warning_state: None,
        stream_tx: None,
        message_rx: None,
        run_tasks: crate::test_run_tasks(),
    };
    let deferred_calls = ["call-question-2", "call-question-3"]
        .into_iter()
        .map(|call_id| {
            FunctionCall::new(
                call_id,
                "AskUserQuestion",
                json!({
                    "header": "Execution Mode",
                    "question": "How should the agent proceed?",
                    "options": [
                        {"id": "plan", "label": "Plan", "description": "Think first"},
                        {"id": "direct", "label": "Direct", "description": "Act now"}
                    ]
                }),
            )
        })
        .collect::<Vec<_>>();
    let second_call = deferred_calls[0].clone();
    let third_call = deferred_calls[1].clone();
    pending.tool_calls.extend(deferred_calls.clone());
    pending.deferred_tool_calls = deferred_calls;
    initial_metrics.tool_calls = 3;
    let result = execute_general_turn_loop_with_resume(
        prepared.clone(),
        Arc::new(NoopLlmProvider),
        tool_registry,
        &config,
        Some(question_resume(pending, initial_metrics)),
    )
    .await
    .expect("first answer should advance to the second question");
    assert_eq!(result.stop_reason, StopReason::WaitingUserAction);
    let pending = result
        .pending_user_action
        .expect("second question should remain pending");
    assert_eq!(pending.call_id, second_call.id);
    assert_eq!(pending.deferred_tool_calls, vec![third_call.clone()]);
    assert_eq!(
        pending.reasoning_continuation,
        Some(ReasoningContinuation::Text(
            "question continuation".to_string()
        ))
    );
    let first_resume_metrics = result.metrics.clone();

    let result = execute_general_turn_loop_with_resume(
        prepared.clone(),
        Arc::new(NoopLlmProvider),
        Arc::new({
            let registry = ToolRegistry::new();
            talos_tools::register_core_local_tools(&registry);
            registry
        }),
        &config,
        Some(question_resume(pending, first_resume_metrics)),
    )
    .await
    .expect("second answer should advance to the third question");
    assert_eq!(result.stop_reason, StopReason::WaitingUserAction);
    let pending = result
        .pending_user_action
        .expect("third question should remain pending");
    assert_eq!(pending.call_id, third_call.id);
    assert!(pending.deferred_tool_calls.is_empty());
    assert_eq!(
        pending.reasoning_continuation,
        Some(ReasoningContinuation::Text(
            "question continuation".to_string()
        ))
    );
    let second_resume_metrics = result.metrics.clone();

    let registry = Arc::new(ToolRegistry::new());
    talos_tools::register_core_local_tools(registry.as_ref());
    let result = execute_general_turn_loop_with_resume(
        prepared,
        Arc::new(QuestionProvider {
            mode: QuestionProviderMode::Resume,
            round: AtomicUsize::new(0),
        }),
        registry,
        &config,
        Some(question_resume(pending, second_resume_metrics)),
    )
    .await
    .expect("third answer should continue the turn");
    assert_eq!(result.output_text, "Proceeding with a plan.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert_eq!(result.metrics.tool_calls, 3);
    assert_eq!(result.metrics.tool_successes, 3);
    assert_eq!(result.metrics.tool_errors, 0);
    assert_eq!(result.metrics.model_rounds, 2);
}

fn question_resume(
    pending: PendingUserActionRequest,
    execution_metrics: TurnExecutionMetrics,
) -> AgentKernelContinuation {
    let PendingUserActionRequest {
        call_id,
        tool_name,
        arguments,
        sandbox_request_fingerprint,
        reasoning_continuation,
        tool_calls,
        tool_messages,
        deferred_tool_calls,
        ..
    } = pending;
    AgentKernelContinuation::UserAction(AgentKernelUserActionContinuation {
        call_id: call_id.clone(),
        tool_name: tool_name.clone(),
        response: talos_types::UserActionResponse::Question(
            talos_types::QuestionAnswer::SelectedOption {
                option_id: "plan".to_string(),
            },
        ),
        sandbox_request_fingerprint,
        original_call: Some(AgentKernelContinuationToolCall {
            item_id: None,
            call_id,
            tool_name,
            arguments: arguments.into(),
            reasoning_continuation,
        }),
        tool_calls,
        tool_messages,
        deferred_tool_calls,
        replay_dynamic_tool_results: HashMap::new(),
        execution_metrics,
    })
}
