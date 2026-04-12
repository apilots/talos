use super::*;
use crate::{
    BeforeModelCallback, BuiltInPlanner, ContextStateBags, InvocationExecutionState,
    InvocationLimits, PlanReActPlanner, ResponseDisposition, TurnPhase,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Role, ToolStream,
    ToolStreamEvent, Usage,
};
use talos_memory::ToolCallRecord;
use talos_tools::{Tool, ToolContext, ToolOutput, ToolRegistry};
use tokio::sync::mpsc;
use uuid::Uuid;

struct NoopLlmProvider;

#[async_trait]
impl LlmProvider for NoopLlmProvider {
    fn name(&self) -> &str {
        "noop"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: String::new(),
            reasoning: Vec::new(),
            model: "noop".to_string(),
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(1);
        Ok(ToolStream::new(rx))
    }

    fn default_model(&self) -> &str {
        "noop"
    }
}

struct StopBeforeModelCallback;

#[async_trait]
impl BeforeModelCallback for StopBeforeModelCallback {
    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _request: &mut LlmRequestEnvelope,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Stop)
    }
}

#[derive(Default)]
struct StepEventTestState {
    chat_round: AtomicUsize,
    tool_payload: Mutex<Option<String>>,
}

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_test"
    }

    fn description(&self) -> &str {
        "Echo tool for flow step tests"
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": { "type": "string" }
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let message = input
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("missing");
        Ok(ToolOutput {
            success: true,
            output: format!("echo:{message}"),
            error: None,
        })
    }
}

struct StepEventMockProvider {
    state: Arc<StepEventTestState>,
}

#[derive(Default)]
struct ApprovalFlowState {
    chat_round: AtomicUsize,
}

struct ApprovalMockProvider {
    state: Arc<ApprovalFlowState>,
}

#[async_trait]
impl LlmProvider for StepEventMockProvider {
    fn name(&self) -> &str {
        "step-event-mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);
        if round == 0 {
            return Ok(ChatResponse {
                content: "Need tool data first.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_1",
                    "echo_test",
                    json!({ "message": "ping" }),
                )]),
            });
        }

        if let Some(message) = request.messages.iter().find(|msg| {
            msg.role == Role::Tool
                && msg.name.as_deref() == Some("echo_test")
                && msg.tool_call_id.as_deref() == Some("call_1")
        }) {
            self.state
                .tool_payload
                .lock()
                .expect("tool payload mutex poisoned")
                .replace(message.content.clone());
        }

        Ok(ChatResponse {
            content: "Final answer after tool.".to_string(),
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

    async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(1);
        Ok(ToolStream::new(rx))
    }
}

#[async_trait]
impl LlmProvider for ApprovalMockProvider {
    fn name(&self) -> &str {
        "approval-mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);
        match round {
            0 => Ok(ChatResponse {
                content: "Need approval first.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_approval",
                    "exec_command",
                    json!({
                        "cmd": "echo hi",
                        "shell": "/bin/sh",
                        "login": false,
                        "sandbox_permissions": "require_escalated",
                        "justification": "Please confirm the risky command."
                    }),
                )]),
            }),
            _ => panic!("approval flow should stop before another model round"),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(1);
        Ok(ToolStream::new(rx))
    }
}

fn build_ctx() -> InvocationContext {
    InvocationContext {
        session_id: "s1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "w1".to_string(),
        input: "hello".to_string(),
        selected_model_info: talos_types::ModelInfo {
            id: "m1".to_string(),
            name: "m1".to_string(),
            provider: "p1".to_string(),
            context_window: 0,
            max_tokens: 0,
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

#[test]
fn test_project_events_includes_final_response() {
    let ctx = build_ctx();
    let events = vec![
        ExecutionEvent::text(
            &ctx,
            EventAuthor::User,
            ExecutionBranch::ReactiveLoop,
            ExecutionEventKind::UserInput,
            "hello",
            false,
        ),
        ExecutionEvent::final_response(&ctx, ExecutionBranch::ReactiveLoop, "done"),
    ];
    assert!(events
        .iter()
        .all(|event| matches!(event.branch, ExecutionBranch::ReactiveLoop)));
    assert!(events
        .iter()
        .any(|event| matches!(event.kind, ExecutionEventKind::FinalResponse)));
}

#[test]
fn test_build_flow_step_model_request_preserves_state_messages() {
    let prepared = crate::general_turn::PreparedGeneralTurn {
        session_id: "s1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        selected_model: "m1".to_string(),
        prompt_prefix_messages: vec![talos_llm_provider::ChatMessage::system("prefix")],
        messages: vec![
            talos_llm_provider::ChatMessage::system("prefix"),
            talos_llm_provider::ChatMessage::user("hello"),
        ],
        tool_definitions: Vec::new(),
        prompt_diagnostics: talos_prompt::PromptDiagnostics {
            mode: talos_prompt::PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            truncated_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
        },
        warnings: Vec::new(),
        stream_tx: None,
    };
    let mut state = GeneralTurnLoopState::new(&prepared);
    state.messages.push(talos_llm_provider::ChatMessage::system(
        "Retrieved memories:\n- MEMORY.md: prefer tea",
    ));

    let request = build_flow_step_model_request(
        None,
        None,
        None,
        ExecutionBranch::ReactiveLoop,
        &prepared,
        &state,
        &KernelExecutionSettings::default(),
    );

    assert_eq!(request.prompt_prefix_messages.len(), 1);
    assert!(request
        .transient_messages
        .iter()
        .any(|message| message.role == Role::System
            && message.content.contains("Retrieved memories:")));
}

#[test]
fn test_build_flow_step_model_request_preserves_native_thinking() {
    let prepared = crate::general_turn::PreparedGeneralTurn {
        session_id: "s1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        selected_model: "m1".to_string(),
        prompt_prefix_messages: Vec::new(),
        messages: Vec::new(),
        tool_definitions: Vec::new(),
        prompt_diagnostics: talos_prompt::PromptDiagnostics {
            mode: talos_prompt::PromptMode::Full,
            included_sections: Vec::new(),
            dropped_sections: Vec::new(),
            truncated_sections: Vec::new(),
            bootstrap_files: Vec::new(),
            total_estimated_tokens: 0,
            static_estimated_tokens: 0,
            dynamic_estimated_tokens: 0,
        },
        warnings: Vec::new(),
        stream_tx: None,
    };
    let state = GeneralTurnLoopState::new(&prepared);
    let request = build_flow_step_model_request(
        None,
        None,
        Some(talos_llm_provider::NativeThinkingConfig {
            effort: talos_llm_provider::NativeThinkingEffort::High,
            budget_tokens: Some(1024),
        }),
        ExecutionBranch::ModelThinking,
        &prepared,
        &state,
        &KernelExecutionSettings::default(),
    );

    assert_eq!(
        request
            .native_thinking
            .as_ref()
            .and_then(|config| config.budget_tokens),
        Some(1024)
    );
}

#[test]
fn test_select_flow_profile_follows_planner_and_tooling() {
    let built_in_settings = KernelExecutionSettings {
        planner_kind: crate::PlannerKind::BuiltIn,
        ..KernelExecutionSettings::default()
    };
    let empty_tools = ToolRegistry::new();
    assert_eq!(
        select_flow_profile(&built_in_settings, &empty_tools),
        FlowProfile::DirectAnswer
    );

    let tool_registry = Arc::new(ToolRegistry::new());
    tool_registry.register(Arc::new(EchoTool));
    assert_eq!(
        select_flow_profile(&built_in_settings, tool_registry.as_ref()),
        FlowProfile::ModelThinking
    );

    let plan_react_settings = KernelExecutionSettings {
        planner_kind: crate::PlannerKind::PlanReAct,
        ..KernelExecutionSettings::default()
    };
    assert_eq!(
        select_flow_profile(&plan_react_settings, tool_registry.as_ref()),
        FlowProfile::ReactiveLoop
    );
}

#[tokio::test]
async fn test_response_pipeline_defaults_to_continue() {
    let pipeline = ResponseProcessorPipeline::new();
    let mut ctx = build_ctx();
    let mut response = LlmResponseEnvelope::default();
    let output = pipeline
        .run(&mut ctx, &mut response)
        .await
        .expect("pipeline should succeed");
    assert_eq!(output.disposition, ResponseDisposition::Continue);
    assert!(output.events.is_empty());
}

#[tokio::test]
async fn test_single_flow_respects_before_model_callback_stop() {
    let callbacks =
        Arc::new(CallbackRegistry::new().with_before_model(Arc::new(StopBeforeModelCallback)));
    let mut ctx = build_ctx();
    let flow = SingleFlow::new(
        "agent-1",
        "test agent",
        Arc::new(NoopLlmProvider),
        Arc::new(ToolRegistry::new()),
        Arc::new(ToolRegistry::new()),
        Arc::new(MemoryStore::new(
            std::env::temp_dir().join("talos-flow-tests"),
        )),
        AgentKernelConfig::default(),
        Arc::new(BuiltInPlanner::new()),
    )
    .with_callbacks(callbacks);

    let result = flow
        .run_events(&mut ctx, "hello")
        .await
        .expect("flow should succeed");

    assert_eq!(
        result.summary.stop_reason,
        crate::StopReason::BeforeModelCallbackStopped
    );
    assert!(result.summary.output_text.is_empty());
    assert!(result.summary.output_items.is_empty());
}

#[tokio::test]
async fn test_single_flow_emits_step_level_events_for_tool_loop() {
    let state = Arc::new(StepEventTestState::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    tool_registry.register(Arc::new(EchoTool));
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let mut ctx = build_ctx();
    let flow = SingleFlow::new(
        "agent-1",
        "test agent",
        Arc::new(StepEventMockProvider {
            state: state.clone(),
        }),
        tool_registry.clone(),
        tool_registry,
        Arc::new(MemoryStore::new(workspace_dir)),
        AgentKernelConfig::default(),
        Arc::new(BuiltInPlanner::new()),
    );

    let result = flow
        .run_events(&mut ctx, "hello")
        .await
        .expect("flow should succeed");

    let kinds = result
        .events
        .iter()
        .map(|event| event.kind.clone())
        .collect::<Vec<_>>();
    let tool_call_index = kinds
        .iter()
        .position(|kind| matches!(kind, ExecutionEventKind::ToolCall))
        .expect("tool call event should exist");
    let tool_result_index = kinds
        .iter()
        .position(|kind| matches!(kind, ExecutionEventKind::ToolResult))
        .expect("tool result event should exist");
    let final_response_index = kinds
        .iter()
        .position(|kind| matches!(kind, ExecutionEventKind::FinalResponse))
        .expect("final response event should exist");
    assert_eq!(kinds.first(), Some(&ExecutionEventKind::UserInput));
    assert!(tool_call_index < tool_result_index);
    assert!(tool_result_index < final_response_index);
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event.kind, ExecutionEventKind::Reasoning)));
    assert_eq!(result.summary.output_text, "Final answer after tool.");
    assert_eq!(state.chat_round.load(Ordering::SeqCst), 2);
    let payload = state
        .tool_payload
        .lock()
        .expect("tool payload mutex poisoned")
        .clone()
        .unwrap_or_default();
    assert!(payload.contains("\"ok\":true"));
}

#[tokio::test]
async fn test_single_flow_stages_confirmation_event_for_pending_tool_approval() {
    let state = Arc::new(ApprovalFlowState::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    talos_tools::register_core_local_tools(tool_registry.as_ref());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let mut ctx = build_ctx();
    let flow = SingleFlow::new(
        "agent-1",
        "test agent",
        Arc::new(ApprovalMockProvider {
            state: state.clone(),
        }),
        tool_registry.clone(),
        tool_registry,
        Arc::new(MemoryStore::new(workspace_dir)),
        AgentKernelConfig::default(),
        Arc::new(BuiltInPlanner::new()),
    );

    let result = flow
        .run_events(&mut ctx, "hello")
        .await
        .expect("flow should succeed");

    assert_eq!(result.summary.output_text, "Tool confirmation pending.");
    assert_eq!(state.chat_round.load(Ordering::SeqCst), 1);
    assert!(result.events.iter().any(|event| matches!(
        &event.payload,
        EventPayload::ConfirmationRequest { payload }
            if payload.tool_name == "exec_command"
                && payload.call_id == "call_approval"
                && payload.prompt == "tool approval is required: Please confirm the risky command."
    )));
    assert!(ctx.session.events.iter().any(|event| matches!(
        &event.payload,
        SessionEventPayload::Confirmation { payload }
            if payload.tool_name == "exec_command"
                && payload.call_id == "call_approval"
                && payload.prompt == "tool approval is required: Please confirm the risky command."
    )));
}

#[test]
fn test_single_flow_current_branch_uses_profile() {
    let built_in_direct = SingleFlow::new(
        "agent-1",
        "test agent",
        Arc::new(NoopLlmProvider),
        Arc::new(ToolRegistry::new()),
        Arc::new(ToolRegistry::new()),
        Arc::new(MemoryStore::new(
            std::env::temp_dir().join(Uuid::new_v4().to_string()),
        )),
        AgentKernelConfig {
            planner_mode: talos_config::PlannerMode::BuiltIn,
            ..AgentKernelConfig::default()
        },
        Arc::new(BuiltInPlanner::new()),
    );
    assert_eq!(
        built_in_direct.current_branch(),
        ExecutionBranch::DirectAnswer
    );

    let tool_registry = Arc::new(ToolRegistry::new());
    tool_registry.register(Arc::new(EchoTool));
    let built_in_model = SingleFlow::new(
        "agent-1",
        "test agent",
        Arc::new(NoopLlmProvider),
        tool_registry.clone(),
        tool_registry.clone(),
        Arc::new(MemoryStore::new(
            std::env::temp_dir().join(Uuid::new_v4().to_string()),
        )),
        AgentKernelConfig {
            planner_mode: talos_config::PlannerMode::BuiltIn,
            ..AgentKernelConfig::default()
        },
        Arc::new(BuiltInPlanner::new()),
    );
    assert_eq!(
        built_in_model.current_branch(),
        ExecutionBranch::ModelThinking
    );

    let plan_react = SingleFlow::new(
        "agent-1",
        "test agent",
        Arc::new(NoopLlmProvider),
        tool_registry.clone(),
        tool_registry,
        Arc::new(MemoryStore::new(
            std::env::temp_dir().join(Uuid::new_v4().to_string()),
        )),
        AgentKernelConfig {
            planner_mode: talos_config::PlannerMode::PlanReAct,
            ..AgentKernelConfig::default()
        },
        Arc::new(PlanReActPlanner::new()),
    );
    assert_eq!(plan_react.current_branch(), ExecutionBranch::ReactiveLoop);
}

#[test]
fn test_should_replan_false_when_replan_budget_exhausted_regardless_of_no_progress_budget() {
    // max_replans = 0 means no replan at all, even when no-progress budget remains.
    let policy = talos_config::PlanReActPolicy {
        max_replans: 0,
        max_no_progress_rounds: 5,
        require_final_answer_tag: true,
    };
    let state = PlanReActLoopState {
        replan_count: 0,
        consecutive_no_progress_rounds: 0,
        consecutive_read_only_rounds: 0,
    };
    assert!(!should_replan(&state, None, &policy));
}

#[test]
fn test_should_replan_false_when_replan_count_reached_max() {
    let policy = talos_config::PlanReActPolicy {
        max_replans: 2,
        max_no_progress_rounds: 5,
        require_final_answer_tag: true,
    };
    let state = PlanReActLoopState {
        replan_count: 2,
        consecutive_no_progress_rounds: 1,
        consecutive_read_only_rounds: 0,
    };
    // replan_count == max_replans: no further replan even if no-progress budget remains
    assert!(!should_replan(&state, None, &policy));
}

#[test]
fn test_should_replan_true_when_no_progress_budget_remains() {
    let policy = talos_config::PlanReActPolicy {
        max_replans: 3,
        max_no_progress_rounds: 2,
        require_final_answer_tag: true,
    };
    let state = PlanReActLoopState {
        replan_count: 1,
        consecutive_no_progress_rounds: 1,
        consecutive_read_only_rounds: 0,
    };
    assert!(should_replan(&state, None, &policy));
}

#[test]
fn test_should_replan_true_when_tool_errors_within_budget() {
    // max_no_progress_rounds = 0, but tool errors still trigger replan via max_replans budget.
    let policy = talos_config::PlanReActPolicy {
        max_replans: 3,
        max_no_progress_rounds: 0,
        require_final_answer_tag: true,
    };
    let state = PlanReActLoopState {
        replan_count: 1,
        consecutive_no_progress_rounds: 0,
        consecutive_read_only_rounds: 0,
    };
    let tool_batch = GeneralTurnToolBatch {
        output_items: Vec::new(),
        tool_messages: Vec::new(),
        pending_confirmations: Vec::new(),
        pending_dynamic_requests: Vec::new(),
        retrieved_memories: Vec::new(),
        logs: Vec::new(),
        error_count: 3, // multiple errors in one batch count as one replan opportunity
    };
    assert!(should_replan(&state, Some(&tool_batch), &policy));
}

#[test]
fn test_should_replan_false_when_tool_errors_but_replan_budget_exhausted() {
    let policy = talos_config::PlanReActPolicy {
        max_replans: 2,
        max_no_progress_rounds: 0,
        require_final_answer_tag: true,
    };
    let state = PlanReActLoopState {
        replan_count: 2,
        consecutive_no_progress_rounds: 0,
        consecutive_read_only_rounds: 0,
    };
    let tool_batch = GeneralTurnToolBatch {
        output_items: Vec::new(),
        tool_messages: Vec::new(),
        pending_confirmations: Vec::new(),
        pending_dynamic_requests: Vec::new(),
        retrieved_memories: Vec::new(),
        logs: Vec::new(),
        error_count: 5,
    };
    // Multiple errors in batch still can't replan — budget exhausted at decision level.
    assert!(!should_replan(&state, Some(&tool_batch), &policy));
}

fn test_loop_state(tool_names: &[&str]) -> GeneralTurnLoopState {
    GeneralTurnLoopState {
        messages: Vec::new(),
        round: 0,
        output_items: Vec::new(),
        usage: talos_types::talos::TalosUsage::default(),
        reasoning_log: Vec::new(),
        tool_call_log: tool_names
            .iter()
            .enumerate()
            .map(|(index, name)| ToolCallRecord {
                call_id: format!("call-{index}"),
                name: (*name).to_string(),
                arguments: json!({}),
            })
            .collect(),
        tool_result_log: Vec::new(),
        retrieved_memories: Vec::new(),
        tool_call_count: tool_names.len(),
        tool_error_count: 0,
        saw_output_delta: false,
    }
}

#[test]
fn test_is_local_path_analysis_request_true_for_absolute_path_prompt() {
    assert!(is_local_path_analysis_request(
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构"
    ));
    assert!(is_local_path_analysis_request(
        "Please analyze /tmp/demo-project architecture"
    ));
}

#[test]
fn test_is_local_path_analysis_request_false_without_analysis_intent() {
    assert!(!is_local_path_analysis_request(
        "Run /Users/xujiang/data/ailabs/PageIndex/run_pageindex.py"
    ));
    assert!(!is_local_path_analysis_request(
        "Please analyze the project architecture"
    ));
}

#[test]
fn test_should_require_local_path_file_probe_until_file_tool_runs() {
    let mut embedded_ctx = build_ctx();
    embedded_ctx.session_environment = Some(talos_types::talos::TalosSessionEnvironment {
        directory: std::path::PathBuf::from("/Users/xujiang/data/ailabs/talos"),
        connection_mode: talos_types::talos::TalosConnectionMode::Embedded,
    });
    let empty_state = test_loop_state(&[]);
    assert!(should_require_local_path_file_probe(
        &embedded_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &empty_state,
    ));

    let probed_state = test_loop_state(&["memory_search", "list_dir"]);
    assert!(!should_require_local_path_file_probe(
        &embedded_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &probed_state,
    ));
}

#[test]
fn test_should_not_require_local_path_file_probe_for_remote_or_missing_environment() {
    let remote_ctx = InvocationContext {
        session_environment: Some(talos_types::talos::TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/Users/xujiang/data/ailabs/talos"),
            connection_mode: talos_types::talos::TalosConnectionMode::Remote,
        }),
        ..build_ctx()
    };
    let none_ctx = build_ctx();
    let state = test_loop_state(&[]);

    assert!(!should_require_local_path_file_probe(
        &remote_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &state,
    ));
    assert!(!should_require_local_path_file_probe(
        &none_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &state,
    ));
}

#[test]
fn test_should_require_structure_probe_for_embedded_without_grep_files() {
    let embedded_ctx = InvocationContext {
        session_environment: Some(talos_types::talos::TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/Users/xujiang/data/ailabs/talos"),
            connection_mode: talos_types::talos::TalosConnectionMode::Embedded,
        }),
        ..build_ctx()
    };
    let state = test_loop_state(&["list_dir", "read_file"]);

    assert!(should_require_structure_probe(
        &embedded_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &state,
    ));
}

#[test]
fn test_should_not_require_structure_probe_after_grep_files() {
    let embedded_ctx = InvocationContext {
        session_environment: Some(talos_types::talos::TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/Users/xujiang/data/ailabs/talos"),
            connection_mode: talos_types::talos::TalosConnectionMode::Embedded,
        }),
        ..build_ctx()
    };
    let state = test_loop_state(&["list_dir", "grep_files", "read_file"]);

    assert!(!should_require_structure_probe(
        &embedded_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &state,
    ));
}

#[test]
fn test_should_require_structure_probe_for_remote_when_file_probe_exists() {
    let remote_ctx = InvocationContext {
        session_environment: Some(talos_types::talos::TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/Users/xujiang/data/ailabs/talos"),
            connection_mode: talos_types::talos::TalosConnectionMode::Remote,
        }),
        ..build_ctx()
    };
    let state = test_loop_state(&["list_dir", "read_file"]);

    assert!(should_require_structure_probe(
        &remote_ctx,
        "分析 /Users/xujiang/data/ailabs/PageIndex 项目架构",
        &state,
    ));
}

#[test]
fn test_is_read_only_tool_round_true_for_read_only_calls() {
    let step = GeneralTurnStepResult {
        text: String::new(),
        reasoning: Vec::new(),
        tool_calls: vec![
            FunctionCall::new("1", "read_file", json!({"path":"a"})),
            FunctionCall::new("2", "web_search", json!({"query":"x"})),
        ],
        output_items: Vec::new(),
        usage: talos_types::talos::TalosUsage::default(),
        saw_output_delta: false,
        disposition: GeneralTurnStepDisposition::ContinueWithTools,
    };
    assert!(is_read_only_tool_round(&step));
}

#[test]
fn test_is_read_only_tool_round_false_for_exec_command() {
    let step = GeneralTurnStepResult {
        text: String::new(),
        reasoning: Vec::new(),
        tool_calls: vec![FunctionCall::new("1", "exec_command", json!({"cmd":"ls"}))],
        output_items: Vec::new(),
        usage: talos_types::talos::TalosUsage::default(),
        saw_output_delta: false,
        disposition: GeneralTurnStepDisposition::ContinueWithTools,
    };
    assert!(!is_read_only_tool_round(&step));
}
