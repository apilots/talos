use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tracing::info;
use uuid::Uuid;

use crate::{
    flush_manager::MemoryFlushManager,
    service::{
        OrchestratorMessageCompletedEvent, OrchestratorMessageErrorEvent,
        OrchestratorMessageExecutionSummaryEvent, OrchestratorMessageOutputDeltaEvent,
        OrchestratorMessageReasoningDeltaEvent, OrchestratorMessageStartedEvent,
        OrchestratorMessageToolCallCompletedEvent, OrchestratorMessageToolCallPendingApprovalEvent,
        OrchestratorMessageToolCallStartedEvent,
    },
    Agent, AgentOrchestrator, AgentRequest, AgentResponse, OrchestratorEvent,
    OrchestratorExecution, OrchestratorRequest,
};
use serde_json::Value;
use talos_agent_kernel::{
    flush_session_memory, Agent as KernelAgent, AgentRunResult, ConfirmationRequestStreamData,
    InvocationContext, KernelRuntime, KernelStreamEvent, KernelStreamEventData,
    KernelStreamEventType, KernelThinkingSource, KernelThinkingStage, LlmKernelAgent,
    LoopKernelAgent, ParallelKernelAgent, PlannerSelector, RunRequest, SequentialKernelAgent,
    StopReason, TaskKind, TaskResult, TaskStatus,
};
use talos_config::{AgentKernelConfig, ExecutionMode, PlannerMode};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, FunctionCall, LlmProvider};
use talos_memory::{
    Memory, MemoryStore, SessionTranscriptActions, SessionTranscriptApprovalResponsePayload,
    SessionTranscriptAuthor, SessionTranscriptBranch, SessionTranscriptDynamicToolResponsePayload,
    SessionTranscriptEventKind, SessionTranscriptHeader, SessionTranscriptPayload,
    SessionTranscriptRecord,
};
use talos_tools::ToolRegistry;
use talos_types::talos::{
    StopReason as TalosStopReason, TalosContentItem, TalosInputItem, TalosOutputItem,
    TalosReasoning, TalosThinkingSource, TalosThinkingStage, TalosUsage,
};
use talos_types::{ModelInfo, TalosSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KernelExecutionMode {
    Single,
    Sequential,
    Parallel,
    Loop,
}

#[derive(Clone)]
struct ActiveTurnHandle {
    execution_task: AbortHandle,
}

#[derive(Clone)]
pub struct AgentImpl {
    pub id: String,
    pub name: String,
    pub description: String,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub model: String,
    pub memory_store: Arc<MemoryStore>,
    pub tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
    pub kernel_runtime: Arc<KernelRuntime>,
    active_turns: Arc<DashMap<String, ActiveTurnHandle>>,
    memory_flush_manager: Arc<MemoryFlushManager>,
}

impl AgentImpl {
    fn resolve_selected_model_info(&self, session: &TalosSession) -> ModelInfo {
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.clone()
        } else {
            session.config.llm.model.clone()
        };
        let provider_name = session.config.llm.provider.clone();

        if let Some(provider_config) = session.config.current_provider_config() {
            if let Some(model) = provider_config
                .models
                .iter()
                .find(|m| m.id == selected_model)
            {
                return ModelInfo {
                    id: model.id.clone(),
                    name: model.display_name().to_string(),
                    provider: provider_name,
                    context_window: model.context_window,
                    max_tokens: model.max_tokens,
                    reasoning: model.reasoning,
                    input: model.input.clone(),
                    cost: model.cost.clone(),
                    compat: model.compat.clone(),
                };
            }
        }

        ModelInfo {
            id: selected_model.clone(),
            name: selected_model,
            provider: provider_name,
            context_window: 0,
            max_tokens: 0,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        }
    }

    async fn validate_dynamic_tool_submission(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
    ) -> TalosResult<()> {
        let records = self
            .memory_store
            .load_transcript_records(session_id, None, SessionTranscriptBranch::ReactiveLoop)
            .await?;

        let mut latest_request_timestamp = None;
        let mut latest_request_tool_name = None;
        let mut latest_response_timestamp = None;

        for record in records {
            match record.payload {
                SessionTranscriptPayload::DynamicToolRequest { payload }
                    if payload.call_id == call_id =>
                {
                    latest_request_timestamp = Some(record.header.timestamp);
                    latest_request_tool_name = Some(payload.tool_name);
                }
                SessionTranscriptPayload::DynamicToolResponse { payload }
                    if payload.call_id == call_id =>
                {
                    latest_response_timestamp = Some(record.header.timestamp);
                }
                _ => {}
            }
        }

        let Some(request_timestamp) = latest_request_timestamp else {
            return Err(TalosError::ToolError(format!(
                "dynamic tool call `{call_id}` is not pending for session `{session_id}`"
            )));
        };

        if latest_request_tool_name.as_deref() != Some(tool_name) {
            return Err(TalosError::ToolError(format!(
                "dynamic tool call `{call_id}` belongs to `{}` not `{tool_name}`",
                latest_request_tool_name.unwrap_or_else(|| "unknown".to_string())
            )));
        }

        if latest_response_timestamp.is_some_and(|ts| ts > request_timestamp) {
            return Err(TalosError::ToolError(format!(
                "dynamic tool call `{call_id}` has already been resolved"
            )));
        }

        let timeout_ms = self.kernel_config.tools.dynamic.response_timeout_ms;
        if timeout_ms > 0 {
            let elapsed_ms = (Utc::now() - request_timestamp).num_milliseconds().max(0) as u64;
            if elapsed_ms >= timeout_ms {
                return Err(TalosError::ToolError(format!(
                    "dynamic tool call `{call_id}` timed out after {timeout_ms} ms"
                )));
            }
        }

        Ok(())
    }

    fn map_stop_reason(stop_reason: StopReason) -> TalosStopReason {
        match stop_reason {
            StopReason::EndTurn => TalosStopReason::EndTurn,
            StopReason::ToolRoundLimit => TalosStopReason::ToolRoundLimit,
            StopReason::TransferUnresolved => TalosStopReason::TransferUnresolved,
            StopReason::BeforeModelCallbackStopped => TalosStopReason::BeforeModelCallbackStopped,
            StopReason::AfterModelCallbackStopped => TalosStopReason::AfterModelCallbackStopped,
        }
    }

    fn register_active_turn(&self, session_id: &str, handle: ActiveTurnHandle) {
        if let Some(previous) = self.active_turns.insert(session_id.to_string(), handle) {
            previous.execution_task.abort();
        }
    }

    fn clear_active_turn(&self, session_id: &str) {
        self.active_turns.remove(session_id);
    }

    fn extract_text_input(input: &[TalosInputItem]) -> String {
        input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::Text { text } => {
                    let normalized = text.trim();
                    if normalized.is_empty() {
                        None
                    } else {
                        Some(normalized.to_string())
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn collect_reasoning(output_items: &[TalosOutputItem]) -> Vec<TalosReasoning> {
        output_items
            .iter()
            .filter_map(|item| match item {
                TalosOutputItem::Reasoning { text } => Some(TalosReasoning { text: text.clone() }),
                _ => None,
            })
            .collect()
    }

    fn to_agent_request(&self, request: OrchestratorRequest) -> AgentRequest {
        AgentRequest {
            session: request.session,
            input: request.input,
            phase: talos_agent_kernel::TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
        }
    }

    fn normalized_kernel_config(mut kernel_config: AgentKernelConfig) -> AgentKernelConfig {
        if kernel_config.binding.default_agent_kind.trim().is_empty() {
            kernel_config.binding.default_agent_kind = "llm".to_string();
        }
        if kernel_config.binding.default_agent_kind.trim() != "llm" {
            kernel_config.binding.default_agent_kind = "llm".to_string();
        }
        if kernel_config.memory.workspace_dir.trim().is_empty() {
            kernel_config.memory.workspace_dir = AgentKernelConfig::default().memory.workspace_dir;
        }

        kernel_config.max_iters = kernel_config.max_iters.max(1);
        kernel_config.max_task_per_turn = kernel_config.max_task_per_turn.max(1);
        kernel_config.tool_parallelism = kernel_config.tool_parallelism.max(1);
        kernel_config.max_llm_calls = kernel_config.max_llm_calls.max(1);
        kernel_config.max_tool_rounds = kernel_config.max_tool_rounds.max(1);
        kernel_config.tool_result_max_bytes = kernel_config.tool_result_max_bytes.max(1024);
        kernel_config.context_compaction_max_bytes =
            kernel_config.context_compaction_max_bytes.max(8 * 1024);
        kernel_config.prompt.max_static_tokens = kernel_config.prompt.max_static_tokens.max(1);
        kernel_config.prompt.max_dynamic_tokens = kernel_config.prompt.max_dynamic_tokens.max(1);
        kernel_config.memory.hot_recent_messages = kernel_config.memory.hot_recent_messages.max(1);
        kernel_config.memory.session_recent_messages_limit =
            kernel_config.memory.session_recent_messages_limit.max(1);
        kernel_config.memory.session_summary_recent_count =
            kernel_config.memory.session_summary_recent_count.max(1);
        kernel_config.memory.session_summary_max_chars =
            kernel_config.memory.session_summary_max_chars.max(128);
        kernel_config.memory.compaction_threshold_bytes =
            kernel_config.memory.compaction_threshold_bytes.max(1024);
        kernel_config.memory.search_max_results = kernel_config.memory.search_max_results.max(1);
        kernel_config
    }

    fn build_memory_store_from_kernel_config(
        kernel_config: &AgentKernelConfig,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> MemoryStore {
        MemoryStore::from_config_with_llm(kernel_config, llm_provider)
    }

    fn rebuild_memory_store(&mut self) {
        self.memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &self.kernel_config,
            self.llm_provider.clone(),
        ));
    }

    pub fn new(
        id: String,
        name: String,
        description: String,
        llm_provider: Arc<dyn LlmProvider>,
        model: String,
        tool_registry: Arc<ToolRegistry>,
        kernel_config: AgentKernelConfig,
    ) -> Self {
        let kernel_config = Self::normalized_kernel_config(kernel_config);
        let memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &kernel_config,
            llm_provider.clone(),
        ));

        Self {
            id,
            name,
            description,
            llm_provider,
            model,
            memory_store,
            tool_registry,
            kernel_config,
            kernel_runtime: Arc::new(KernelRuntime::new_default()),
            active_turns: Arc::new(DashMap::new()),
            memory_flush_manager: Arc::new(MemoryFlushManager::new()),
        }
    }

    pub async fn drain_pending_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.memory_flush_manager.drain(timeout).await
    }

    pub fn pending_memory_flush_count(&self) -> usize {
        self.memory_flush_manager.pending_count()
    }

    /// Add a tool to this agent's registry
    pub fn add_tool(&self, tool: Arc<dyn talos_tools::Tool>) {
        self.tool_registry.register(tool);
    }

    /// Set the tool registry for this agent (allows external tool injection)
    pub fn set_tool_registry(&mut self, tool_registry: Arc<ToolRegistry>) {
        self.tool_registry = tool_registry;
    }

    pub fn set_default_planner_mode(&mut self, planner_mode: &str) -> TalosResult<()> {
        let mode = PlannerMode::parse(planner_mode.trim())?;
        self.kernel_config.planner_mode = mode;
        Ok(())
    }

    pub fn set_prompt_budget(&mut self, max_static_tokens: usize, max_dynamic_tokens: usize) {
        self.kernel_config.prompt.max_static_tokens = max_static_tokens.max(1);
        self.kernel_config.prompt.max_dynamic_tokens = max_dynamic_tokens.max(1);
    }

    /// Update kernel configuration with a new config object.
    pub fn update_kernel_config(&mut self, config: AgentKernelConfig) {
        self.kernel_config = Self::normalized_kernel_config(config);
        self.rebuild_memory_store();
    }

    /// Execute native function calls and produce tool-role messages for next LLM round.
    #[allow(dead_code)]
    async fn process_tool_calls(
        &self,
        tool_calls: Vec<FunctionCall>,
    ) -> TalosResult<Vec<ChatMessage>> {
        let mut tool_messages = Vec::new();
        let context = self
            .tool_registry
            .build_context(
                format!("agent-{}", self.id),
                self.name.clone(),
                Some(Arc::new(self.kernel_config_snapshot())),
            )
            .with_memory_store(self.memory_store.clone());

        for tool_call in tool_calls {
            tracing::info!(
                "Agent {} executing tool call: {} with args: {:?}",
                self.id,
                tool_call.name,
                tool_call.arguments
            );

            let invocation = self.tool_registry.build_function_invocation(
                format!("agent-call-{}", tool_call.id),
                &tool_call.name,
                tool_call.arguments.clone(),
            )?;
            match self
                .tool_registry
                .execute_invocation_with_context(invocation, context.clone())
                .await
            {
                Ok(tool_result) => {
                    let content_json = match tool_result {
                        talos_tools::ToolDispatchOutcome::Executed { payload } => {
                            if payload.is_error() {
                                serde_json::json!({
                                    "ok": false,
                                    "tool": tool_call.name,
                                    "error": payload.error_message().unwrap_or_else(|| "unknown error".to_string()),
                                })
                            } else {
                                serde_json::json!({
                                    "ok": true,
                                    "tool": tool_call.name,
                                    "result": payload.content(),
                                })
                            }
                        }
                        talos_tools::ToolDispatchOutcome::Pending { pending, .. } => {
                            match pending {
                                talos_tools::ToolPending::Approval { prompt, .. } => {
                                    serde_json::json!({
                                        "ok": false,
                                        "tool": tool_call.name,
                                        "error": prompt,
                                    })
                                }
                                talos_tools::ToolPending::Dynamic { .. } => serde_json::json!({
                                    "ok": false,
                                    "tool": tool_call.name,
                                    "error": "dynamic tool response is pending",
                                }),
                            }
                        }
                    };

                    tool_messages.push(ChatMessage::tool(
                        tool_call.id.clone(),
                        tool_call.name.clone(),
                        content_json.to_string(),
                    ));
                }
                Err(e) => {
                    let err_json = serde_json::json!({
                        "ok": false,
                        "tool": tool_call.name,
                        "error": e.to_string(),
                    });

                    tool_messages.push(ChatMessage::tool(
                        tool_call.id.clone(),
                        tool_call.name.clone(),
                        err_json.to_string(),
                    ));
                }
            }
        }

        Ok(tool_messages)
    }

    #[allow(dead_code)]
    fn derive_exec_command(goal: &str) -> String {
        let trimmed = goal.trim();
        let normalized = trimmed.to_ascii_lowercase();
        for prefix in ["run ", "execute ", "exec "] {
            if normalized.starts_with(prefix) {
                let cmd = trimmed[prefix.len()..].trim();
                if !cmd.is_empty() {
                    return cmd.to_string();
                }
            }
        }
        trimmed.to_string()
    }

    /// Generate a system prompt that includes tool information
    pub fn generate_system_prompt(&self) -> String {
        let available_tools = self.tool_registry.list_tool_names();
        format!(
            r#"You are {}, {}

You have access to the following tools:
{}

Use native function calling when a tool is needed.
After receiving tool outputs, continue reasoning and provide the final response."#,
            self.name,
            self.description,
            available_tools.join(", ")
        )
    }

    fn build_llm_kernel_agent(
        &self,
        id: impl Into<String>,
        name: impl Into<String>,
    ) -> Arc<dyn KernelAgent> {
        let planner_selector = Arc::new(PlannerSelector::new(self.llm_provider.clone()));
        Arc::new(LlmKernelAgent::new(
            id,
            name,
            self.description.clone(),
            self.llm_provider.clone(),
            self.memory_store.clone(),
            self.kernel_config_snapshot(),
            self.tool_registry.clone(),
            self.tool_registry.clone(),
            planner_selector,
        ))
    }

    fn kernel_config_snapshot(&self) -> AgentKernelConfig {
        self.kernel_config.clone()
    }

    fn resolve_kernel_mode(&self) -> TalosResult<KernelExecutionMode> {
        let mode = self.kernel_config.execution_mode;
        Ok(match mode {
            ExecutionMode::Single => KernelExecutionMode::Single,
            ExecutionMode::Sequential => KernelExecutionMode::Sequential,
            ExecutionMode::Parallel => KernelExecutionMode::Parallel,
            ExecutionMode::Loop => KernelExecutionMode::Loop,
        })
    }

    fn build_kernel_agent_for_request(&self) -> TalosResult<Arc<dyn KernelAgent>> {
        let mode = self.resolve_kernel_mode()?;
        let child_count = self.kernel_config.execution_children.clamp(1, 4);
        let loop_iters = self.kernel_config.execution_loop_iters.clamp(1, 16);

        Ok(match mode {
            KernelExecutionMode::Single => {
                self.build_llm_kernel_agent(self.id.clone(), self.name.clone())
            }
            KernelExecutionMode::Sequential => {
                let children = (0..child_count)
                    .map(|idx| {
                        self.build_llm_kernel_agent(
                            format!("{}-seq-{}", self.id, idx + 1),
                            format!("{} seq {}", self.name, idx + 1),
                        )
                    })
                    .collect::<Vec<_>>();
                Arc::new(SequentialKernelAgent::new(
                    format!("{}-sequential", self.id),
                    children,
                ))
            }
            KernelExecutionMode::Parallel => {
                let children = (0..child_count)
                    .map(|idx| {
                        self.build_llm_kernel_agent(
                            format!("{}-par-{}", self.id, idx + 1),
                            format!("{} par {}", self.name, idx + 1),
                        )
                    })
                    .collect::<Vec<_>>();
                Arc::new(ParallelKernelAgent::new(
                    format!("{}-parallel", self.id),
                    children,
                ))
            }
            KernelExecutionMode::Loop => Arc::new(LoopKernelAgent::new(
                format!("{}-loop", self.id),
                self.build_llm_kernel_agent(
                    format!("{}-loop-body", self.id),
                    format!("{} loop body", self.name),
                ),
                loop_iters,
            )),
        })
    }

    fn map_kernel_stream_event(
        event: KernelStreamEvent,
        request_id: &str,
        item_index: &mut u64,
    ) -> Option<OrchestratorEvent> {
        let current_index = *item_index;
        let iteration = current_index.saturating_add(1);
        let step_id = format!("step-{}", iteration);
        let mapped = match (event.event_type, event.data) {
            (KernelStreamEventType::ThinkingDelta, KernelStreamEventData::ThinkingDelta(data)) => {
                Some(OrchestratorEvent::MessageReasoningDelta(
                    OrchestratorMessageReasoningDeltaEvent {
                        request_id: request_id.to_string(),
                        delta: data.delta,
                        source: map_thinking_source(data.source),
                        stage: map_thinking_stage(data.stage),
                        item_index: current_index,
                        iteration,
                        step_id,
                    },
                ))
            }
            (
                KernelStreamEventType::ExecutionSummary,
                KernelStreamEventData::ExecutionSummary(data),
            ) => Some(OrchestratorEvent::MessageExecutionSummary(
                OrchestratorMessageExecutionSummaryEvent {
                    request_id: request_id.to_string(),
                    text: data.text,
                    item_index: current_index,
                    iteration,
                    step_id,
                },
            )),
            (KernelStreamEventType::OutputDelta, KernelStreamEventData::OutputDelta(data)) => Some(
                OrchestratorEvent::MessageOutputDelta(OrchestratorMessageOutputDeltaEvent {
                    request_id: request_id.to_string(),
                    delta: data.delta,
                    item_index: current_index,
                    iteration,
                    step_id,
                }),
            ),
            (KernelStreamEventType::ToolCallStart, KernelStreamEventData::ToolCallStart(data)) => {
                Some(OrchestratorEvent::MessageToolCallStarted(
                    OrchestratorMessageToolCallStartedEvent {
                        request_id: request_id.to_string(),
                        call_id: data.call_id.unwrap_or_else(|| "unknown".to_string()),
                        name: data.tool_name.unwrap_or_else(|| "unknown".to_string()),
                        arguments: data.arguments,
                        item_index: current_index,
                        iteration,
                        step_id,
                        timing_ms: None,
                    },
                ))
            }
            (KernelStreamEventType::ToolResult, KernelStreamEventData::ToolResult(data)) => {
                Some(OrchestratorEvent::MessageToolCallCompleted(
                    OrchestratorMessageToolCallCompletedEvent {
                        request_id: request_id.to_string(),
                        call_id: data.call_id,
                        tool: data.tool_name,
                        content: if data.content.is_empty() {
                            vec![TalosContentItem::Json {
                                value: Value::Array(Vec::new()),
                            }]
                        } else {
                            data.content
                        },
                        is_error: data.is_error,
                        item_index: current_index,
                        iteration,
                        step_id,
                        timing_ms: None,
                    },
                ))
            }
            (KernelStreamEventType::PromptWarning, KernelStreamEventData::PromptWarning(data)) => {
                Some(OrchestratorEvent::MessageExecutionSummary(
                    OrchestratorMessageExecutionSummaryEvent {
                        request_id: request_id.to_string(),
                        text: format!("Prompt warning: {:?}", data.warning),
                        item_index: current_index,
                        iteration,
                        step_id,
                    },
                ))
            }
            (
                KernelStreamEventType::ConfirmationRequest,
                KernelStreamEventData::ConfirmationRequest(ConfirmationRequestStreamData {
                    call_id,
                    tool_name,
                    prompt,
                    arguments,
                }),
            ) => Some(OrchestratorEvent::MessageToolCallPendingApproval(
                OrchestratorMessageToolCallPendingApprovalEvent {
                    request_id: request_id.to_string(),
                    call_id,
                    tool_name,
                    prompt,
                    arguments,
                    step_id,
                },
            )),
            _ => None,
        };
        if mapped.is_some() {
            *item_index = (*item_index).saturating_add(1);
        }
        mapped
    }

    /// Submit a user's approval or rejection decision for a pending tool confirmation.
    ///
    /// Persists an `ApprovalResponse` event to the session transcript. On the next
    /// invocation of this session the `RequestConfirmationRequestProcessor` will detect
    /// the response and either replay the blocked tool call (if approved) or inject a
    /// rejection message so the LLM can surface the outcome to the user.
    pub async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        let record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: Uuid::new_v4().to_string(),
                branch: SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: SessionTranscriptAuthor::System,
            kind: SessionTranscriptEventKind::SystemMessage,
            payload: SessionTranscriptPayload::ApprovalResponse {
                payload: SessionTranscriptApprovalResponsePayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    approved,
                    reason,
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        self.memory_store.append_transcript_record(&record).await
    }

    pub async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.validate_dynamic_tool_submission(session_id, call_id, tool_name)
            .await?;
        let record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: Uuid::new_v4().to_string(),
                branch: SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: SessionTranscriptAuthor::System,
            kind: SessionTranscriptEventKind::SystemMessage,
            payload: SessionTranscriptPayload::DynamicToolResponse {
                payload: SessionTranscriptDynamicToolResponsePayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output,
                    success,
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        self.memory_store.append_transcript_record(&record).await
    }
}

fn map_thinking_source(source: KernelThinkingSource) -> TalosThinkingSource {
    match source {
        KernelThinkingSource::Provider => TalosThinkingSource::Provider,
        KernelThinkingSource::Framework => TalosThinkingSource::Framework,
    }
}

fn map_thinking_stage(stage: KernelThinkingStage) -> TalosThinkingStage {
    match stage {
        KernelThinkingStage::ModelReasoning => TalosThinkingStage::ModelReasoning,
        KernelThinkingStage::FrameworkPlan => TalosThinkingStage::FrameworkPlan,
        KernelThinkingStage::FrameworkAct => TalosThinkingStage::FrameworkAct,
        KernelThinkingStage::FrameworkObserve => TalosThinkingStage::FrameworkObserve,
        KernelThinkingStage::FrameworkReflect => TalosThinkingStage::FrameworkReflect,
    }
}

#[async_trait]
impl Agent for AgentImpl {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> TalosResult<AgentResponse> {
        tracing::info!(
            "Agent {} executing tool: {} with args: {:?}",
            self.id,
            tool_name,
            args
        );

        // Execute the tool through the registry
        let tool_context = self
            .tool_registry
            .build_context(
                format!("agent-{}", self.id),
                self.name.clone(),
                Some(Arc::new(self.kernel_config_snapshot())),
            )
            .with_memory_store(self.memory_store.clone());
        let invocation = self.tool_registry.build_function_invocation(
            format!("agent-direct-{tool_name}"),
            tool_name,
            args,
        )?;
        let tool_result = self
            .tool_registry
            .execute_invocation_with_context(invocation, tool_context)
            .await?;

        let content = match tool_result {
            talos_tools::ToolDispatchOutcome::Executed { payload } => {
                if payload.is_error() {
                    format!(
                        "Tool execution failed: {}",
                        payload
                            .error_message()
                            .unwrap_or_else(|| "Unknown error".to_string())
                    )
                } else {
                    payload.content().to_string()
                }
            }
            talos_tools::ToolDispatchOutcome::Pending { pending, .. } => match pending {
                talos_tools::ToolPending::Approval { prompt, .. } => {
                    format!("Tool execution pending approval: {prompt}")
                }
                talos_tools::ToolPending::Dynamic { .. } => {
                    "Tool execution is waiting for a dynamic tool response".to_string()
                }
            },
        };

        Ok(AgentResponse {
            output_text: content,
            output_items: Vec::new(),
            reasoning: Vec::new(),
            tool_calls: vec![], // Tool calls from within a tool execution context would go here
        })
    }
}

#[async_trait]
impl AgentOrchestrator for AgentImpl {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        let agent_request = self.to_agent_request(request);
        let input = Self::extract_text_input(&agent_request.input);
        let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
        let session_id = agent_request.session.context.session_id.clone();
        let selected_model_info = self.resolve_selected_model_info(&agent_request.session);
        let phase = agent_request.phase.clone();
        let planning_instruction = agent_request.planning_instruction.clone();
        let transfer_to = agent_request.transfer_to.clone();
        let kernel_agent = self.build_kernel_agent_for_request()?;
        let runtime = self.kernel_runtime.clone();
        let agent_id = self.id.clone();
        let kernel_config = self.kernel_config.clone();
        let run_session_id = session_id.clone();
        let run_handle = tokio::spawn({
            async move {
                runtime
                    .run_turn(
                        kernel_agent,
                        RunRequest {
                            session_id: run_session_id,
                            invocation_id,
                            agent_id,
                            workspace_id: agent_request
                                .session
                                .context
                                .workspace_id
                                .clone()
                                .unwrap_or_default(),
                            input,
                            selected_model_info,
                            phase,
                            planning_instruction,
                            transfer_to,
                            initial_state_bags: None,
                            session_environment: agent_request.session.context.environment.clone(),
                            kernel_config: Some(kernel_config),
                            stream_tx: None,
                        },
                    )
                    .await
            }
        });
        self.register_active_turn(
            &session_id,
            ActiveTurnHandle {
                execution_task: run_handle.abort_handle(),
            },
        );
        let result = match run_handle.await {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => {
                self.clear_active_turn(&session_id);
                return Err(err);
            }
            Err(err) if err.is_cancelled() => {
                self.clear_active_turn(&session_id);
                return Ok(OrchestratorExecution {
                    output_text: String::new(),
                    output_items: Vec::new(),
                    reasoning: Vec::new(),
                    stop_reason: TalosStopReason::Cancelled,
                    usage: TalosUsage::default(),
                    task_count: 0,
                    session_id,
                });
            }
            Err(err) => {
                self.clear_active_turn(&session_id);
                return Err(TalosError::InternalError(format!(
                    "kernel task join failed: {}",
                    err
                )));
            }
        };
        self.clear_active_turn(&session_id);
        let reasoning = Self::collect_reasoning(&result.output_items);
        Ok(OrchestratorExecution {
            output_text: result.output_text,
            output_items: result.output_items,
            reasoning,
            stop_reason: Self::map_stop_reason(result.stop_reason),
            usage: result.usage,
            task_count: result.task_results.len() as u32,
            session_id,
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let this = self.clone();
        let stream_task = tokio::spawn(async move {
            let agent_request = this.to_agent_request(request);
            let input = Self::extract_text_input(&agent_request.input);
            let request_id = format!("req_{}", Uuid::new_v4().simple());
            let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
            let session_id = agent_request.session.context.session_id.clone();
            let model = if agent_request.session.config.llm.model.is_empty() {
                this.model.clone()
            } else {
                agent_request.session.config.llm.model.clone()
            };
            let selected_model_info = this.resolve_selected_model_info(&agent_request.session);
            let phase = agent_request.phase.clone();
            let planning_instruction = agent_request.planning_instruction.clone();
            let transfer_to = agent_request.transfer_to.clone();
            let kernel_agent = match this.build_kernel_agent_for_request() {
                Ok(agent) => agent,
                Err(err) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageError(
                            OrchestratorMessageErrorEvent {
                                request_id: request_id.clone(),
                                error: err.to_string(),
                            },
                        ))
                        .await;
                    return;
                }
            };
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let runtime = this.kernel_runtime.clone();
            let kernel_input = RunRequest {
                session_id: agent_request.session.context.session_id.clone(),
                invocation_id,
                agent_id: this.id.clone(),
                workspace_id: agent_request
                    .session
                    .context
                    .workspace_id
                    .clone()
                    .unwrap_or_default(),
                input,
                selected_model_info,
                phase,
                planning_instruction,
                transfer_to,
                initial_state_bags: None,
                session_environment: agent_request.session.context.environment.clone(),
                kernel_config: Some(this.kernel_config.clone()),
                stream_tx: Some(kernel_tx),
            };

            let _ = tx
                .send(OrchestratorEvent::MessageStarted(
                    OrchestratorMessageStartedEvent {
                        request_id: request_id.clone(),
                        model,
                        usage: TalosUsage::default(),
                        iteration: 0,
                        step_id: "start".to_string(),
                    },
                ))
                .await;

            let run_handle =
                tokio::spawn(async move { runtime.run_turn(kernel_agent, kernel_input).await });
            this.register_active_turn(
                &session_id,
                ActiveTurnHandle {
                    execution_task: run_handle.abort_handle(),
                },
            );

            let mut item_index = 0_u64;
            while let Some(kernel_event) = kernel_rx.recv().await {
                if let Some(event) =
                    Self::map_kernel_stream_event(kernel_event, &request_id, &mut item_index)
                {
                    let _ = tx.send(event).await;
                }
            }

            match run_handle.await {
                Ok(Ok(result)) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageCompleted(
                            OrchestratorMessageCompletedEvent {
                                request_id: request_id.clone(),
                                stop_reason: Self::map_stop_reason(result.stop_reason),
                                usage: result.usage,
                                total_output_items: item_index,
                                iteration: item_index.saturating_add(1),
                                step_id: "completed".to_string(),
                                task_count: result.task_results.len() as u32,
                            },
                        ))
                        .await;
                }
                Err(err) if err.is_cancelled() => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageCompleted(
                            OrchestratorMessageCompletedEvent {
                                request_id: request_id.clone(),
                                stop_reason: TalosStopReason::Cancelled,
                                usage: TalosUsage::default(),
                                total_output_items: item_index,
                                iteration: item_index.saturating_add(1),
                                step_id: "cancelled".to_string(),
                                task_count: 0,
                            },
                        ))
                        .await;
                }
                Ok(Err(err)) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageError(
                            OrchestratorMessageErrorEvent {
                                request_id: request_id.clone(),
                                error: err.to_string(),
                            },
                        ))
                        .await;
                }
                Err(err) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageError(
                            OrchestratorMessageErrorEvent {
                                request_id: request_id.clone(),
                                error: format!("kernel stream task join failed: {}", err),
                            },
                        ))
                        .await;
                }
            }
            this.clear_active_turn(&session_id);
        });
        drop(stream_task);
        Ok(rx)
    }

    async fn close_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.clone()
        } else {
            session.config.llm.model.clone()
        };
        let session_id = session.context.session_id.clone();
        let task_session_id = session_id.clone();
        let llm_provider = self.llm_provider.clone();
        let memory_store = self.memory_store.clone();
        let kernel_config = self.kernel_config.clone();
        let accepted = self
            .memory_flush_manager
            .spawn_flush(&session_id, async move {
                flush_session_memory(
                    &task_session_id,
                    &selected_model,
                    llm_provider,
                    memory_store.as_ref(),
                    &kernel_config,
                    false,
                    false,
                    kernel_config.memory.workspace_memory_flush_enabled,
                )
                .await
            })
            .await;
        if !accepted {
            info!(session_id = %session_id, "memory flush deduplicated for session");
        }
        Ok(())
    }

    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.drain_pending_memory_flushes(timeout).await
    }

    fn pending_memory_flushes(&self) -> usize {
        self.pending_memory_flush_count()
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let Some((_, handle)) = self.active_turns.remove(session_id) else {
            return Ok(());
        };
        handle.execution_task.abort();
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        AgentImpl::submit_tool_approval(self, session_id, call_id, tool_name, approved, reason)
            .await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        AgentImpl::submit_dynamic_tool_result(self, session_id, call_id, tool_name, output, success)
            .await
    }
}

// Mock implementation for testing
pub struct MockAgent;

impl MockAgent {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Agent for MockAgent {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_tool(
        &self,
        _tool_name: &str,
        _args: serde_json::Value,
    ) -> TalosResult<AgentResponse> {
        Ok(AgentResponse {
            output_text: "Mock tool execution".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "Mock tool execution".to_string(),
            }],
            reasoning: Vec::new(),
            tool_calls: vec![],
        })
    }
}

#[async_trait]
impl KernelAgent for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _event_handler: Option<&mut dyn talos_agent_kernel::ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult> {
        Ok(AgentRunResult {
            output_text: "Mock response".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "Mock response".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            warnings: Vec::new(),
            task_results: vec![TaskResult {
                task_id: "mock-task".to_string(),
                kind: TaskKind::General,
                goal: "mock".to_string(),
                status: TaskStatus::Success,
                output: "Mock response".to_string(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: "Mock response".to_string(),
                }],
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                tool_traces: Vec::new(),
                state_delta: HashMap::new(),
                error: None,
            }],
            state_delta: HashMap::new(),
            continue_loop: false,
            requested_transfer: None,
        })
    }
}

#[async_trait]
impl AgentOrchestrator for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        let input = AgentImpl::extract_text_input(&request.input);
        let output = format!("Mock response to: {}", input);
        Ok(OrchestratorExecution {
            output_text: output,
            output_items: vec![TalosOutputItem::OutputText {
                text: "Mock response".to_string(),
            }],
            reasoning: Vec::new(),
            stop_reason: TalosStopReason::EndTurn,
            usage: TalosUsage::default(),
            task_count: 1,
            session_id: request.session.context.session_id,
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let execution = AgentOrchestrator::run(self, request).await?;
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let (tx, rx) = mpsc::channel(8);
        tokio::spawn(async move {
            let _ = tx
                .send(OrchestratorEvent::MessageStarted(
                    OrchestratorMessageStartedEvent {
                        request_id: request_id.clone(),
                        model: "mock-model".to_string(),
                        usage: TalosUsage::default(),
                        iteration: 0,
                        step_id: "start".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageOutputDelta(
                    OrchestratorMessageOutputDeltaEvent {
                        request_id: request_id.clone(),
                        delta: execution.output_text,
                        item_index: 0,
                        iteration: 1,
                        step_id: "step-1".to_string(),
                    },
                ))
                .await;
            let _ = tx
                .send(OrchestratorEvent::MessageCompleted(
                    OrchestratorMessageCompletedEvent {
                        request_id,
                        stop_reason: execution.stop_reason,
                        usage: execution.usage,
                        total_output_items: 1,
                        iteration: 2,
                        step_id: "completed".to_string(),
                        task_count: execution.task_count,
                    },
                ))
                .await;
        });
        Ok(rx)
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_memory_flushes(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    fn pending_memory_flushes(&self) -> usize {
        0
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _approved: bool,
        _reason: Option<String>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use serde_json::json;
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use talos_agent_kernel::{KernelBuilder, RunRequest, RunResponse, Runner};
    use talos_config::AgentKernelConfig;
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Role, Usage};
    use talos_memory::{Memory, TalosTurn};
    use talos_tools::{ReadFileTool, ToolRegistry, WebSearchTool};
    use talos_tools_policy::ToolPolicy;
    use talos_types::talos::{
        StopReason as TalosStopReason, TalosOutputItem, TalosProvider, TalosSourceType, TalosUsage,
    };
    use talos_types::{SessionContext, SessionState};
    use tempfile::TempDir;
    use tokio;
    use tokio::sync::Notify;

    fn build_test_agent() -> AgentImpl {
        AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(MockLlmProvider {}),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        )
    }

    struct SequencedRunner {
        started: Arc<Notify>,
        calls: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Runner for SequencedRunner {
        async fn run_turn(
            &self,
            _agent: Arc<dyn talos_agent_kernel::Agent>,
            _request: RunRequest,
        ) -> TalosResult<RunResponse> {
            let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
            if call_index == 0 {
                self.started.notify_waiters();
                pending::<()>().await;
                unreachable!("pending future should not resolve");
            }

            Ok(RunResponse {
                output_text: "runner ok".to_string(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: "runner ok".to_string(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                warnings: Vec::new(),
                model: "mock-model".to_string(),
                task_results: Vec::new(),
                transfer_to: None,
                iterations: 1,
                events: Vec::new(),
            })
        }
    }

    fn build_temp_agent() -> (TempDir, AgentImpl) {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let mut kernel_config = AgentKernelConfig::default();
        kernel_config.memory.workspace_dir = temp_dir.path().display().to_string();
        kernel_config.memory.compaction_threshold_bytes = usize::MAX;
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(MockLlmProvider {}),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            kernel_config,
        );
        (temp_dir, agent)
    }

    fn build_test_session(session_id: &str, workspace_id: &str) -> TalosSession {
        TalosSession {
            title: "Untitled Session".to_string(),
            context: SessionContext {
                session_id: session_id.to_string(),
                workspace_id: Some(workspace_id.to_string()),
                source_type: TalosSourceType::Cli,
                provider: TalosProvider::Cli,
                user_id: "local-user".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: None,
                thread_id: None,
                environment: None,
            },
            config: talos_config::Config::default(),
            state: SessionState {
                created_at: 1,
                last_activity: 1,
                message_count: 2,
                usage: TalosUsage::default(),
            },
        }
    }

    #[tokio::test]
    async fn test_process_tool_calls() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let policy = ToolPolicy::allow_list(vec!["web_search"]);
        let registry = Arc::new(ToolRegistry::new_with_policy(policy));
        registry.register(Arc::new(WebSearchTool));
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            registry,
            Default::default(),
        );

        // Create a tool call to test
        let tool_call = FunctionCall {
            id: "call_123".to_string(),
            name: "web_search".to_string(),
            arguments: json!({"query": "test"}),
        };

        // Process the tool call
        let result = agent.process_tool_calls(vec![tool_call]).await;

        assert!(result.is_ok());
        let messages = result.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, Role::Tool);
    }

    #[tokio::test]
    async fn test_generate_system_prompt() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        );

        // Add some tools to the registry
        agent.add_tool(Arc::new(WebSearchTool));
        agent.add_tool(Arc::new(ReadFileTool));

        // Generate system prompt
        let prompt = agent.generate_system_prompt();

        assert!(prompt.contains("Test Agent"));
        assert!(prompt.contains("An agent for testing"));
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("read_file"));
    }

    #[tokio::test]
    async fn test_add_tool() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            Arc::new(ToolRegistry::new()),
            Default::default(),
        );

        // Verify initial state
        assert_eq!(agent.tool_registry.list_tool_names().len(), 0);

        // Add a tool
        agent.add_tool(Arc::new(WebSearchTool));

        // Verify the tool was added
        let tools = agent.tool_registry.list_tool_names();
        assert!(tools.contains(&"web_search".to_string()));
    }

    #[tokio::test]
    async fn test_execute_tool() {
        // Create an agent with a mock provider
        let mock_provider = MockLlmProvider {};
        let policy = ToolPolicy::allow_list(vec!["web_search"]);
        let registry = Arc::new(ToolRegistry::new_with_policy(policy));
        registry.register(Arc::new(WebSearchTool));
        let agent = AgentImpl::new(
            "test_agent".to_string(),
            "Test Agent".to_string(),
            "An agent for testing".to_string(),
            Arc::new(mock_provider),
            "gpt-4o".to_string(),
            registry,
            Default::default(),
        );

        // Execute the tool
        let result = agent
            .execute_tool("web_search", json!({"query": "test"}))
            .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.output_text.contains("Search results for 'test'"));
    }

    #[tokio::test]
    async fn test_close_session_flushes_workspace_memory() {
        let (_guard, agent) = build_temp_agent();
        let session_id = "session-close-flush";
        let session = build_test_session(session_id, "workspace-a");
        let timestamp = Utc::now();
        let turn = TalosTurn {
            turn_id: "turn-1".to_string(),
            session_id: session_id.to_string(),
            seq: 1,
            timestamp,
            items: vec![
                TalosOutputItem::UserInput {
                    text: "Remember that I prefer tea over coffee.".to_string(),
                },
                TalosOutputItem::OutputText {
                    text: "Noted. You prefer tea over coffee.".to_string(),
                },
            ],
            usage: TalosUsage::default(),
            stop_reason: TalosStopReason::EndTurn,
        };

        agent
            .memory_store
            .append_turn(&turn)
            .await
            .expect("session turn should be persisted");

        agent
            .close_session(session)
            .await
            .expect("session close should enqueue workspace memory flush");
        agent
            .drain_pending_memory_flushes(Duration::from_secs(5))
            .await
            .expect("memory flush should complete during drain");

        let file_path = _guard
            .path()
            .join("memory")
            .join(format!("{}.md", Local::now().format("%Y-%m-%d")));
        let mut content = None;
        for _ in 0..20 {
            match tokio::fs::read_to_string(&file_path).await {
                Ok(value) => {
                    content = Some(value);
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(25)).await,
            }
        }
        let content = content.expect("workspace daily memory file should exist");
        assert!(content.contains("## ["));
        assert!(content.contains("session:"));
        assert!(content.contains("Durable Memory"));
        assert!(!content.trim().is_empty());
    }

    #[test]
    fn test_derive_exec_command() {
        assert_eq!(AgentImpl::derive_exec_command("run echo ok"), "echo ok");
        assert_eq!(AgentImpl::derive_exec_command("execute ls"), "ls");
        assert_eq!(AgentImpl::derive_exec_command("exec pwd"), "pwd");
        assert_eq!(AgentImpl::derive_exec_command("echo plain"), "echo plain");
    }

    #[test]
    fn test_planner_mode_comes_from_kernel_config() {
        let mut agent = build_test_agent();
        agent
            .set_default_planner_mode("plan_react")
            .expect("planner mode should parse");
        assert_eq!(
            agent.kernel_config.planner_mode,
            talos_config::PlannerMode::PlanReAct
        );
    }

    #[test]
    fn test_kernel_binding_default_is_llm() {
        let agent = build_test_agent();
        assert_eq!(agent.kernel_config.binding.default_agent_kind, "llm");
    }

    #[tokio::test]
    async fn test_submit_dynamic_tool_result_rejects_replay() {
        let (_temp_dir, agent) = build_temp_agent();
        let session_id = "session-dynamic-replay";
        let request_record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: Uuid::new_v4().to_string(),
                branch: SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: SessionTranscriptAuthor::System,
            kind: SessionTranscriptEventKind::SystemMessage,
            payload: SessionTranscriptPayload::DynamicToolRequest {
                payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                    call_id: "call-1".to_string(),
                    tool_name: "skill_greet".to_string(),
                    arguments: json!({ "name": "Talos" }),
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        agent
            .memory_store
            .append_transcript_record(&request_record)
            .await
            .expect("request record should append");

        agent
            .submit_dynamic_tool_result(
                session_id,
                "call-1",
                "skill_greet",
                "hello".to_string(),
                true,
            )
            .await
            .expect("first submission should succeed");

        let error = agent
            .submit_dynamic_tool_result(
                session_id,
                "call-1",
                "skill_greet",
                "hello again".to_string(),
                true,
            )
            .await
            .expect_err("second submission should be rejected");
        assert!(error.to_string().contains("already been resolved"));
    }

    #[tokio::test]
    async fn test_submit_dynamic_tool_result_rejects_timeout() {
        let (_temp_dir, mut agent) = build_temp_agent();
        agent.kernel_config.tools.dynamic.response_timeout_ms = 1;
        let session_id = "session-dynamic-timeout";
        let request_record = SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: Uuid::new_v4().to_string(),
                branch: SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now() - chrono::Duration::milliseconds(10),
            },
            author: SessionTranscriptAuthor::System,
            kind: SessionTranscriptEventKind::SystemMessage,
            payload: SessionTranscriptPayload::DynamicToolRequest {
                payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                    call_id: "call-timeout".to_string(),
                    tool_name: "skill_greet".to_string(),
                    arguments: json!({}),
                },
            },
            actions: SessionTranscriptActions::default(),
            partial: false,
        };
        agent
            .memory_store
            .append_transcript_record(&request_record)
            .await
            .expect("request record should append");

        let error = agent
            .submit_dynamic_tool_result(
                session_id,
                "call-timeout",
                "skill_greet",
                "late".to_string(),
                true,
            )
            .await
            .expect_err("timed out submission should be rejected");
        assert!(error.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_cancel_active_turn_aborts_only_current_session_turn() {
        let mut agent = build_test_agent();
        let started = Arc::new(Notify::new());
        agent.kernel_runtime = Arc::new(
            KernelBuilder::new()
                .with_runner(Arc::new(SequencedRunner {
                    started: started.clone(),
                    calls: AtomicUsize::new(0),
                }))
                .build(),
        );
        let session = build_test_session("session-cancel", "workspace-a");

        let mut first_stream = agent
            .run_stream(OrchestratorRequest {
                session: session.clone(),
                input: vec![TalosInputItem::Text {
                    text: "first".to_string(),
                }],
            })
            .await
            .expect("first stream should start");
        started.notified().await;
        agent
            .cancel_active_turn(&session.context.session_id)
            .await
            .expect("cancel should succeed");

        let mut first_stop_reason = None;
        while let Some(event) = first_stream.recv().await {
            if let OrchestratorEvent::MessageCompleted(payload) = event {
                first_stop_reason = Some(payload.stop_reason);
                break;
            }
        }
        assert_eq!(first_stop_reason, Some(TalosStopReason::Cancelled));

        let mut second_stream = agent
            .run_stream(OrchestratorRequest {
                session,
                input: vec![TalosInputItem::Text {
                    text: "second".to_string(),
                }],
            })
            .await
            .expect("second stream should start");
        let mut second_stop_reason = None;
        while let Some(event) = second_stream.recv().await {
            if let OrchestratorEvent::MessageCompleted(payload) = event {
                second_stop_reason = Some(payload.stop_reason);
                break;
            }
        }
        assert_eq!(second_stop_reason, Some(TalosStopReason::EndTurn));
    }

    // Mock LLM provider for testing
    struct MockLlmProvider;

    #[async_trait::async_trait]
    impl LlmProvider for MockLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }

        fn default_model(&self) -> &str {
            "mock-model"
        }

        async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            let content = if request.messages.iter().any(|message| {
                message
                    .content
                    .contains("Extract durable memory from this conversation as JSON.")
            }) {
                "{\"title\":\"Durable Memory\",\"facts\":[\"User prefers tea over coffee\"]}"
                    .to_string()
            } else {
                "Test response".to_string()
            };
            Ok(ChatResponse {
                content,
                reasoning: Vec::new(),
                model: "mock-model".to_string(),
                usage: Usage { ..Usage::default() },
                finish_reason: None,
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (tx, rx) = tokio::sync::mpsc::channel(100);
            drop(tx); // Close the sender for mock
            Ok(ChatStream { rx })
        }
    }
}
