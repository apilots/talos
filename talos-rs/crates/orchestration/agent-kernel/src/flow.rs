use crate::general_turn::{
    apply_general_turn_step, apply_general_turn_tool_batch, build_general_turn_step_request,
    emit_general_turn_finalization_stream, execute_general_turn_step,
    execute_general_turn_tool_batch, finalize_general_turn_artifacts,
    persist_general_turn_artifacts, prepare_general_turn, GeneralTurnLoopState,
    GeneralTurnStepDisposition,
};
use crate::{
    CallbackControl, CallbackRegistry, EventAuthor, EventSink, ExecutionBranch, ExecutionEvent,
    ExecutionEventBuffer, ExecutionEventKind, FlowKind, InvocationContext, KernelExecutionSettings,
    KernelWarning, LlmRequestEnvelope, LlmResponseEnvelope, MemoryBackedSessionService, Planner,
    RequestProcessorPipeline, ResponseProcessorPipeline, SessionEvent, SessionEventKind,
    SessionService, StopReason, ToolCallbackPayload, ToolExecutor, ToolExecutorFactory,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::talos::{TalosContentItem, TalosOutputItem, TalosUsage};

#[derive(Debug, Clone)]
pub struct FlowRunResult {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub warnings: Vec<KernelWarning>,
    pub events: Vec<ExecutionEvent>,
}

#[async_trait]
pub trait BaseLlmFlow: Send + Sync {
    async fn run(&self, ctx: &mut InvocationContext, input: &str) -> TalosResult<FlowRunResult>;
}

pub struct SingleFlow {
    agent_name: String,
    agent_description: String,
    llm_provider: Arc<dyn LlmProvider>,
    tool_registry: Arc<ToolRegistry>,
    memory_store: Arc<MemoryStore>,
    kernel_config: AgentKernelConfig,
    #[allow(dead_code)]
    planner: Arc<dyn Planner>,
    settings: KernelExecutionSettings,
    request_pipeline: RequestProcessorPipeline,
    response_pipeline: ResponseProcessorPipeline,
    callbacks: Arc<CallbackRegistry>,
    tool_executor: Arc<dyn ToolExecutor>,
    session_service: Arc<dyn SessionService>,
}

impl SingleFlow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        tool_registry: Arc<ToolRegistry>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        planner: Arc<dyn Planner>,
    ) -> Self {
        let settings = KernelExecutionSettings::try_from(&kernel_config).unwrap_or_default();
        let tool_executor = ToolExecutorFactory::build(settings.tool_execution_mode);
        Self {
            agent_name: agent_name.into(),
            agent_description: agent_description.into(),
            llm_provider,
            tool_registry,
            memory_store: memory_store.clone(),
            kernel_config,
            planner: planner.clone(),
            settings,
            request_pipeline: RequestProcessorPipeline::default_single(planner.clone()),
            response_pipeline: ResponseProcessorPipeline::default_single(planner),
            callbacks: Arc::new(CallbackRegistry::default()),
            tool_executor,
            session_service: Arc::new(MemoryBackedSessionService::new(memory_store.clone())),
        }
    }

    pub fn settings(&self) -> &KernelExecutionSettings {
        &self.settings
    }

    pub fn tool_execution_mode(&self) -> crate::ToolExecutionMode {
        self.tool_executor.mode()
    }

    pub fn current_branch(&self) -> ExecutionBranch {
        match self.settings.thinking_mode {
            crate::ThinkingMode::ModelNative => ExecutionBranch::ModelThinking,
            crate::ThinkingMode::PlanningProcessor | crate::ThinkingMode::Disabled => {
                self.settings.default_path.into()
            }
        }
    }

    pub fn with_callbacks(mut self, callbacks: Arc<CallbackRegistry>) -> Self {
        self.callbacks = callbacks;
        self
    }

    pub fn with_tool_executor(mut self, tool_executor: Arc<dyn ToolExecutor>) -> Self {
        self.tool_executor = tool_executor;
        self
    }

    pub fn with_session_service(mut self, session_service: Arc<dyn SessionService>) -> Self {
        self.session_service = session_service;
        self
    }

    async fn observe_tool_callbacks(
        &self,
        ctx: &mut InvocationContext,
        output_items: &[TalosOutputItem],
    ) -> TalosResult<()> {
        for item in output_items {
            match item {
                TalosOutputItem::ToolCall { call_id, name, .. } => {
                    let payload = ToolCallbackPayload {
                        tool_name: name.clone(),
                        call_id: call_id.clone(),
                    };
                    if matches!(
                        self.callbacks.run_before_tool(ctx, &payload).await?,
                        CallbackControl::Stop
                    ) {
                        break;
                    }
                }
                TalosOutputItem::ToolResult { call_id, .. } => {
                    let tool_name = output_items.iter().find_map(|candidate| match candidate {
                        TalosOutputItem::ToolCall {
                            call_id: candidate_call_id,
                            name,
                            ..
                        } if candidate_call_id == call_id => Some(name.clone()),
                        TalosOutputItem::ToolResult {
                            call_id: candidate_call_id,
                            content,
                            ..
                        } if candidate_call_id == call_id => {
                            content.iter().find_map(|entry| match entry {
                                TalosContentItem::Json { value } => value
                                    .get("tool")
                                    .and_then(|item| item.as_str())
                                    .map(str::to_string),
                                TalosContentItem::Text { .. } => None,
                            })
                        }
                        _ => None,
                    });
                    if let Some(tool_name) = tool_name {
                        let payload = ToolCallbackPayload {
                            tool_name,
                            call_id: call_id.clone(),
                        };
                        if matches!(
                            self.callbacks.run_after_tool(ctx, &payload).await?,
                            CallbackControl::Stop
                        ) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn build_flow_step_model_request(
    planning_instruction: Option<String>,
    branch: ExecutionBranch,
    state: &GeneralTurnLoopState,
) -> LlmRequestEnvelope {
    LlmRequestEnvelope {
        planning_instruction,
        messages: state.messages.clone(),
        branch,
    }
}

#[async_trait]
impl BaseLlmFlow for SingleFlow {
    async fn run(&self, ctx: &mut InvocationContext, input: &str) -> TalosResult<FlowRunResult> {
        let mut sink = ExecutionEventBuffer::new();
        let branch = self.current_branch();
        ctx.session = self
            .session_service
            .load_session(&ctx.session_id, None, Some(branch))
            .await?;
        sink.push(ExecutionEvent::text(
            ctx,
            EventAuthor::User,
            branch,
            ExecutionEventKind::UserInput,
            ctx.input.clone(),
            false,
        ))
        .await?;
        let mut request = LlmRequestEnvelope {
            planning_instruction: ctx.planning_instruction.clone(),
            branch,
            ..LlmRequestEnvelope::default()
        };
        self.request_pipeline
            .run(ctx, &mut request, &mut sink)
            .await?;
        if matches!(
            self.callbacks.run_before_model(ctx, &mut request).await?,
            CallbackControl::Stop
        ) {
            let stop_reason = if sink.has_final_response(branch) {
                StopReason::EndTurn
            } else {
                StopReason::BeforeModelCallbackStopped
            };
            return Ok(FlowRunResult {
                output_text: String::new(),
                output_items: Vec::new(),
                stop_reason,
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                events: sink.into_events(),
            });
        }

        let prepared = prepare_general_turn(
            ctx,
            &self.agent_name,
            &self.agent_description,
            input,
            self.tool_registry.clone(),
            self.memory_store.clone(),
            &self.kernel_config,
        )
        .await?;
        let warnings = prepared.warnings.clone();
        let _ = self
            .session_service
            .append_event(
                &mut ctx.session,
                SessionEvent::new(
                    prepared.session_id.clone(),
                    prepared.invocation_id.clone(),
                    request.branch,
                    EventAuthor::User,
                    SessionEventKind::UserMessage,
                    Some(talos_llm_provider::ChatMessage::user(input)),
                ),
            )
            .await?;
        let max_tool_rounds = self.kernel_config.max_tool_rounds.max(1);
        let mut state = GeneralTurnLoopState::new(&prepared);
        let mut final_text = String::new();
        let stop_reason = loop {
            let mut step_request = build_general_turn_step_request(&prepared, &state);
            let mut step_model_request = build_flow_step_model_request(
                request.planning_instruction.clone(),
                request.branch,
                &state,
            );
            if matches!(
                self.callbacks
                    .run_before_model(ctx, &mut step_model_request)
                    .await?,
                CallbackControl::Stop
            ) {
                let stop_reason = if sink.has_final_response(branch) {
                    StopReason::EndTurn
                } else {
                    StopReason::BeforeModelCallbackStopped
                };
                return Ok(FlowRunResult {
                    output_text: final_text,
                    output_items: state.output_items.clone(),
                    stop_reason,
                    usage: state.usage,
                    warnings,
                    events: sink.into_events(),
                });
            }
            step_request.messages = step_model_request.messages;
            let mut step =
                execute_general_turn_step(step_request, self.llm_provider.clone()).await?;
            let mut step_response = LlmResponseEnvelope {
                output_text: step.text.clone(),
                output_items: step.output_items.clone(),
                branch: request.branch,
            };
            if matches!(
                self.callbacks
                    .run_after_model(ctx, &mut step_response)
                    .await?,
                CallbackControl::Stop
            ) {
                let stop_reason = if sink.has_final_response(branch) {
                    StopReason::EndTurn
                } else {
                    StopReason::AfterModelCallbackStopped
                };
                return Ok(FlowRunResult {
                    output_text: step_response.output_text,
                    output_items: step_response.output_items,
                    stop_reason,
                    usage: state.usage,
                    warnings,
                    events: sink.into_events(),
                });
            }
            step.text = step_response.output_text.clone();
            step.output_items = step_response.output_items.clone();

            let mut projected_step_response = step_response;
            if matches!(step.disposition, GeneralTurnStepDisposition::FinalResponse) {
                projected_step_response.output_text.clear();
            }
            let _ = self
                .response_pipeline
                .run(ctx, &mut projected_step_response, &mut sink)
                .await?;
            self.observe_tool_callbacks(ctx, &step.output_items).await?;
            apply_general_turn_step(&mut state, &step);

            if matches!(step.disposition, GeneralTurnStepDisposition::FinalResponse) {
                final_text = step.text.clone();
                let _ = self
                    .session_service
                    .append_event(
                        &mut ctx.session,
                        SessionEvent::new(
                            prepared.session_id.clone(),
                            prepared.invocation_id.clone(),
                            request.branch,
                            EventAuthor::Agent,
                            SessionEventKind::AgentMessage,
                            Some(talos_llm_provider::ChatMessage::assistant(
                                final_text.clone(),
                            )),
                        ),
                    )
                    .await?;
                state
                    .messages
                    .push(talos_llm_provider::ChatMessage::assistant(
                        final_text.clone(),
                    ));
                break StopReason::EndTurn;
            }
            let _ = self
                .session_service
                .append_event(
                    &mut ctx.session,
                    SessionEvent::new(
                        prepared.session_id.clone(),
                        prepared.invocation_id.clone(),
                        request.branch,
                        EventAuthor::Agent,
                        SessionEventKind::ToolCall,
                        Some(talos_llm_provider::ChatMessage::assistant_with_tool_calls(
                            step.text.clone(),
                            step.tool_calls.clone(),
                        )),
                    ),
                )
                .await?;

            let tool_batch = execute_general_turn_tool_batch(
                &prepared,
                self.tool_executor.clone(),
                self.tool_registry.clone(),
                Arc::new(self.kernel_config.clone()),
                &self.agent_name,
                &step.tool_calls,
                prepared.stream_tx.as_ref(),
            )
            .await?;
            let mut tool_response = LlmResponseEnvelope {
                output_text: String::new(),
                output_items: tool_batch.output_items.clone(),
                branch: request.branch,
            };
            let _ = self
                .response_pipeline
                .run(ctx, &mut tool_response, &mut sink)
                .await?;
            self.observe_tool_callbacks(ctx, &tool_response.output_items)
                .await?;
            for output_item in &tool_response.output_items {
                if let TalosOutputItem::ToolResult {
                    call_id, content, ..
                } = output_item
                {
                    let tool_name = content.iter().find_map(|entry| match entry {
                        TalosContentItem::Json { value } => value
                            .get("tool")
                            .and_then(|item| item.as_str())
                            .map(str::to_string),
                        TalosContentItem::Text { .. } => None,
                    });
                    if let Some(tool_name) = tool_name {
                        let payload = content
                            .iter()
                            .find_map(|entry| match entry {
                                TalosContentItem::Json { value } => Some(value.to_string()),
                                TalosContentItem::Text { text } => Some(text.clone()),
                            })
                            .unwrap_or_else(|| "{}".to_string());
                        let _ = self
                            .session_service
                            .append_event(
                                &mut ctx.session,
                                SessionEvent::new(
                                    prepared.session_id.clone(),
                                    prepared.invocation_id.clone(),
                                    request.branch,
                                    EventAuthor::Tool,
                                    SessionEventKind::ToolResult,
                                    Some(talos_llm_provider::ChatMessage::tool(
                                        call_id.clone(),
                                        tool_name,
                                        payload,
                                    )),
                                ),
                            )
                            .await?;
                    }
                }
            }
            apply_general_turn_tool_batch(&mut state, &tool_batch, &self.kernel_config);

            if state.round >= max_tool_rounds {
                final_text = "[tool-call rounds exceeded]".to_string();
                break StopReason::ToolRoundLimit;
            }
        };

        let executed = finalize_general_turn_artifacts(state, final_text.clone(), stop_reason);
        emit_general_turn_finalization_stream(&prepared, &executed).await;
        let mut final_response = LlmResponseEnvelope {
            output_text: executed.output_text.clone(),
            output_items: executed
                .output_items
                .iter()
                .rev()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            branch: request.branch,
        };
        let _ = self
            .response_pipeline
            .run(ctx, &mut final_response, &mut sink)
            .await?;
        persist_general_turn_artifacts(
            &prepared,
            input,
            &executed,
            self.llm_provider.clone(),
            self.memory_store.clone(),
            &self.kernel_config,
        )
        .await;

        let events = sink.into_events();

        Ok(FlowRunResult {
            output_text: final_response.output_text,
            output_items: executed.output_items,
            stop_reason,
            usage: executed.usage,
            warnings,
            events,
        })
    }
}

pub struct AutoFlow {
    inner: SingleFlow,
}

impl AutoFlow {
    pub fn new(mut inner: SingleFlow) -> Self {
        inner.request_pipeline = RequestProcessorPipeline::default_auto(inner.planner.clone());
        inner.response_pipeline = ResponseProcessorPipeline::default_auto(inner.planner.clone());
        Self { inner }
    }

    pub fn inner(&self) -> &SingleFlow {
        &self.inner
    }
}

#[async_trait]
impl BaseLlmFlow for AutoFlow {
    async fn run(&self, ctx: &mut InvocationContext, input: &str) -> TalosResult<FlowRunResult> {
        self.inner.run(ctx, input).await
    }
}

impl From<crate::ExecutionPath> for ExecutionBranch {
    fn from(value: crate::ExecutionPath) -> Self {
        match value {
            crate::ExecutionPath::DirectAnswer => Self::DirectAnswer,
            crate::ExecutionPath::ReactiveLoop => Self::ReactiveLoop,
        }
    }
}

pub struct FlowFactory;

impl FlowFactory {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        tool_registry: Arc<ToolRegistry>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        planner: Arc<dyn Planner>,
    ) -> Box<dyn BaseLlmFlow> {
        let settings = KernelExecutionSettings::try_from(&kernel_config).unwrap_or_default();
        let single = SingleFlow::new(
            agent_name,
            agent_description,
            llm_provider,
            tool_registry,
            memory_store,
            kernel_config,
            planner,
        );

        match settings.flow_kind {
            FlowKind::Single => Box::new(single),
            FlowKind::Auto => Box::new(AutoFlow::new(single)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BeforeModelCallback, BuiltInPlanner, ContextStateBags, ExecutionEventBuffer,
        InvocationExecutionState, InvocationLimits, ResponseDisposition, TurnPhase,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use talos_config::AgentKernelConfig;
    use talos_llm_provider::{
        ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Role,
        ToolStream, ToolStreamEvent, Usage,
    };
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

    fn build_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
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

        let request = build_flow_step_model_request(None, ExecutionBranch::ReactiveLoop, &state);

        assert_eq!(request.messages.len(), 3);
        assert!(request
            .messages
            .iter()
            .any(|message| message.role == Role::System
                && message.content.contains("Retrieved memories:")));
    }

    #[tokio::test]
    async fn test_response_pipeline_defaults_to_continue() {
        let pipeline = ResponseProcessorPipeline::new();
        let mut ctx = build_ctx();
        let mut response = LlmResponseEnvelope::default();
        let mut sink = ExecutionEventBuffer::new();
        let disposition = pipeline
            .run(&mut ctx, &mut response, &mut sink)
            .await
            .expect("pipeline should succeed");
        assert_eq!(disposition, ResponseDisposition::Continue);
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
            Arc::new(MemoryStore::new(
                std::env::temp_dir().join("talos-flow-tests"),
            )),
            AgentKernelConfig::default(),
            Arc::new(BuiltInPlanner::new()),
        )
        .with_callbacks(callbacks);

        let result = flow
            .run(&mut ctx, "hello")
            .await
            .expect("flow should succeed");

        assert_eq!(
            result.stop_reason,
            crate::StopReason::BeforeModelCallbackStopped
        );
        assert!(result.output_text.is_empty());
        assert!(result.output_items.is_empty());
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
            tool_registry,
            Arc::new(MemoryStore::new(workspace_dir)),
            AgentKernelConfig::default(),
            Arc::new(BuiltInPlanner::new()),
        );

        let result = flow
            .run(&mut ctx, "hello")
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
        assert_eq!(result.output_text, "Final answer after tool.");
        assert_eq!(state.chat_round.load(Ordering::SeqCst), 2);
        let payload = state
            .tool_payload
            .lock()
            .expect("tool payload mutex poisoned")
            .clone()
            .unwrap_or_default();
        assert!(payload.contains("\"ok\":true"));
    }
}
