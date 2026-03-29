use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    Agent, AgentOrchestrator, AgentRequest, AgentResponse, OrchestratorEvent,
    OrchestratorExecution, OrchestratorRequest,
};
use serde_json::{json, Value};
use talos_agent_kernel::{
    flush_session_memory, Agent as KernelAgent, AgentRunResult, BuiltInPlanner, InvocationContext,
    KernelOutputDeltaSource, KernelReasoningSource, KernelRuntime, KernelStreamEvent,
    KernelStreamEventData, KernelStreamEventType, LlmKernelAgent, LoopKernelAgent,
    ParallelKernelAgent, PlanReActPlanner, Planner, RoutingKernelAgent, RunRequest,
    SequentialKernelAgent, StopReason, TaskKind, TaskResult, TaskStatus,
};
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, FunctionCall, LlmProvider};
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::talos::{TalosInputItem, TalosOutputItem, TalosReasoning, TalosUsage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KernelExecutionMode {
    Single,
    Sequential,
    Parallel,
    Loop,
    A2A,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlannerMode {
    BuiltIn,
    PlanReAct,
}

impl PlannerMode {
    fn as_str(self) -> &'static str {
        match self {
            PlannerMode::BuiltIn => "built_in",
            PlannerMode::PlanReAct => "plan_react",
        }
    }

    fn parse(value: &str) -> TalosResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "built_in" | "builtin" => Ok(Self::BuiltIn),
            "plan_react" | "planreact" => Ok(Self::PlanReAct),
            other => Err(TalosError::ValidationError(format!(
                "invalid planner mode: {}",
                other
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KernelAgentKind {
    Llm,
}

#[derive(Clone)]
struct KernelAgentRegistry {
    kind_map: HashMap<String, KernelAgentKind>,
    default_kind: String,
}

impl KernelAgentRegistry {
    fn new(default_kind: impl Into<String>) -> Self {
        let mut kind_map = HashMap::new();
        kind_map.insert("llm".to_string(), KernelAgentKind::Llm);
        Self {
            kind_map,
            default_kind: default_kind.into().to_ascii_lowercase(),
        }
    }

    fn default_kind(&self) -> &str {
        &self.default_kind
    }

    fn set_default_kind(&mut self, kind: impl Into<String>) {
        self.default_kind = kind.into().to_ascii_lowercase();
    }

    fn resolve(&self, requested: Option<&str>) -> KernelAgentKind {
        let requested = requested
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_else(|| self.default_kind.clone());
        self.kind_map
            .get(&requested)
            .copied()
            .or_else(|| self.kind_map.get(self.default_kind.as_str()).copied())
            .unwrap_or(KernelAgentKind::Llm)
    }
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
    planner_override: Option<Arc<dyn Planner>>,
    kernel_config: AgentKernelConfig,
    pub kernel_runtime: Arc<KernelRuntime>,
    kernel_agent_registry: KernelAgentRegistry,
}

impl AgentImpl {
    fn map_stop_reason(stop_reason: StopReason) -> &'static str {
        match stop_reason {
            StopReason::EndTurn => "end_turn",
            StopReason::ToolRoundLimit => "tool_round_limit",
            StopReason::TransferUnresolved => "transfer_unresolved",
            StopReason::BeforeModelCallbackStopped => "before_model_callback_stopped",
            StopReason::AfterModelCallbackStopped => "after_model_callback_stopped",
        }
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

    async fn finalize_session_memory(
        &self,
        session: &talos_types::TalosSession,
    ) -> TalosResult<()> {
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.as_str()
        } else {
            session.config.llm.model.as_str()
        };
        flush_session_memory(
            &session.context.session_id,
            selected_model,
            self.llm_provider.clone(),
            &self.memory_store,
            &self.kernel_config,
            false,
            false,
            self.kernel_config.memory.workspace_memory_flush_enabled,
        )
        .await
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
        kernel_config.memory.search_min_score =
            kernel_config.memory.search_min_score.clamp(0.0, 1.0);
        kernel_config
    }

    fn build_memory_store_from_kernel_config(kernel_config: &AgentKernelConfig) -> MemoryStore {
        MemoryStore::from_config(&kernel_config.memory)
    }

    fn rebuild_memory_store(&mut self) {
        self.memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &self.kernel_config,
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
        let memory_store = Arc::new(Self::build_memory_store_from_kernel_config(&kernel_config));
        let mut kernel_agent_registry =
            KernelAgentRegistry::new(kernel_config.binding.default_agent_kind.clone());
        kernel_agent_registry.set_default_kind(kernel_config.binding.default_agent_kind.clone());

        Self {
            id,
            name,
            description,
            llm_provider,
            model,
            memory_store,
            tool_registry,
            planner_override: None,
            kernel_config,
            kernel_runtime: Arc::new(KernelRuntime::new_default()),
            kernel_agent_registry,
        }
    }

    /// Add a tool to this agent's registry
    pub fn add_tool(&self, tool: Arc<dyn talos_tools::Tool>) {
        self.tool_registry.register(tool);
    }

    /// Set the tool registry for this agent (allows external tool injection)
    pub fn set_tool_registry(&mut self, tool_registry: Arc<ToolRegistry>) {
        self.tool_registry = tool_registry;
    }

    /// Set planner for this agent.
    pub fn set_planner(&mut self, planner: Arc<dyn Planner>) {
        self.planner_override = Some(planner);
    }

    pub fn set_default_planner_mode(&mut self, planner_mode: &str) -> TalosResult<()> {
        let mode = PlannerMode::parse(planner_mode.trim())?;
        self.kernel_config.planner_mode = mode.as_str().to_string();
        Ok(())
    }

    pub fn set_allow_request_planner_override(&mut self, allow: bool) {
        self.kernel_config.allow_request_planner_override = allow;
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
        let context = self.tool_registry.build_context(
            format!("agent-{}", self.id),
            self.name.clone(),
            Some(Arc::new(self.kernel_config_snapshot())),
        );

        for tool_call in tool_calls {
            tracing::info!(
                "Agent {} executing tool call: {} with args: {:?}",
                self.id,
                tool_call.name,
                tool_call.arguments
            );

            match self
                .tool_registry
                .execute_with_context(
                    &tool_call.name,
                    tool_call.arguments.clone(),
                    context.clone(),
                )
                .await
            {
                Ok(tool_result) => {
                    let content_json = if tool_result.success {
                        serde_json::json!({
                            "ok": true,
                            "tool": tool_call.name,
                            "result": tool_result.output,
                        })
                    } else {
                        serde_json::json!({
                            "ok": false,
                            "tool": tool_call.name,
                            "error": tool_result.error.unwrap_or_else(|| "unknown error".to_string()),
                        })
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
        planner: Arc<dyn Planner>,
    ) -> Arc<dyn KernelAgent> {
        Arc::new(LlmKernelAgent::new(
            id,
            name,
            self.description.clone(),
            self.llm_provider.clone(),
            self.memory_store.clone(),
            self.kernel_config_snapshot(),
            self.tool_registry.clone(),
            planner,
        ))
    }

    fn kernel_config_snapshot(&self) -> AgentKernelConfig {
        let mut kernel = self.kernel_config.clone();
        kernel.binding.default_agent_kind = self.kernel_agent_registry.default_kind().to_string();
        kernel
    }

    fn build_kernel_agent_by_kind(
        &self,
        kind: KernelAgentKind,
        id: impl Into<String>,
        name: impl Into<String>,
        planner: Arc<dyn Planner>,
    ) -> Arc<dyn KernelAgent> {
        match kind {
            KernelAgentKind::Llm => self.build_llm_kernel_agent(id, name, planner),
        }
    }

    fn resolve_planner_mode(&self) -> TalosResult<PlannerMode> {
        PlannerMode::parse(&self.kernel_config.planner_mode)
    }

    fn build_planner_for_request(&self) -> TalosResult<Arc<dyn Planner>> {
        if let Some(planner) = self.planner_override.as_ref() {
            return Ok(planner.clone());
        }
        Ok(match self.resolve_planner_mode()? {
            PlannerMode::BuiltIn => Arc::new(BuiltInPlanner::new()),
            PlannerMode::PlanReAct => Arc::new(PlanReActPlanner::new()),
        })
    }

    fn resolve_kernel_agent_kind(&self) -> KernelAgentKind {
        self.kernel_agent_registry
            .resolve(Some(self.kernel_agent_registry.default_kind()))
    }

    fn resolve_kernel_mode(&self) -> KernelExecutionMode {
        let mode = self
            .kernel_config
            .execution_mode
            .trim()
            .to_ascii_lowercase();
        match mode.as_str() {
            "sequential" => KernelExecutionMode::Sequential,
            "parallel" => KernelExecutionMode::Parallel,
            "loop" => KernelExecutionMode::Loop,
            "a2a" | "route" | "routing" => KernelExecutionMode::A2A,
            _ => KernelExecutionMode::Single,
        }
    }

    fn build_kernel_agent_for_request(&self) -> TalosResult<Arc<dyn KernelAgent>> {
        let planner = self.build_planner_for_request()?;
        let agent_kind = self.resolve_kernel_agent_kind();
        let mode = self.resolve_kernel_mode();
        let child_count = self.kernel_config.execution_children.clamp(1, 4);
        let loop_iters = self.kernel_config.execution_loop_iters.clamp(1, 16);

        Ok(match mode {
            KernelExecutionMode::Single => self.build_kernel_agent_by_kind(
                agent_kind,
                self.id.clone(),
                self.name.clone(),
                planner.clone(),
            ),
            KernelExecutionMode::Sequential => {
                let children = (0..child_count)
                    .map(|idx| {
                        self.build_kernel_agent_by_kind(
                            agent_kind,
                            format!("{}-seq-{}", self.id, idx + 1),
                            format!("{} seq {}", self.name, idx + 1),
                            planner.clone(),
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
                        self.build_kernel_agent_by_kind(
                            agent_kind,
                            format!("{}-par-{}", self.id, idx + 1),
                            format!("{} par {}", self.name, idx + 1),
                            planner.clone(),
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
                self.build_kernel_agent_by_kind(
                    agent_kind,
                    format!("{}-loop-body", self.id),
                    format!("{} loop body", self.name),
                    planner.clone(),
                ),
                loop_iters,
            )),
            KernelExecutionMode::A2A => {
                let mut router = RoutingKernelAgent::new(format!("{}-routing", self.id), "primary")
                    .with_max_hops(loop_iters);
                router.register_node(
                    "primary",
                    self.build_kernel_agent_by_kind(
                        agent_kind,
                        format!("{}-primary", self.id),
                        format!("{} primary", self.name),
                        planner.clone(),
                    ),
                );
                router.register_node(
                    "peer",
                    self.build_kernel_agent_by_kind(
                        agent_kind,
                        format!("{}-peer", self.id),
                        format!("{} peer", self.name),
                        planner,
                    ),
                );
                Arc::new(router)
            }
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
            (
                KernelStreamEventType::ReasoningDelta,
                KernelStreamEventData::ReasoningDelta(data),
            ) => {
                let source = match data.source {
                    KernelReasoningSource::Provider => "provider".to_string(),
                    KernelReasoningSource::Kernel => "kernel".to_string(),
                };
                Some(OrchestratorEvent {
                    event_type: "message.reasoning.delta".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "delta": data.delta,
                        "reasoning_source": source,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                    }),
                })
            }
            (KernelStreamEventType::OutputDelta, KernelStreamEventData::OutputDelta(data)) => {
                let output_source = match data.source {
                    Some(KernelOutputDeltaSource::Provider) => "provider",
                    Some(KernelOutputDeltaSource::KernelFallback) => "kernel_fallback",
                    None => "unknown",
                };
                Some(OrchestratorEvent {
                    event_type: "message.output.delta".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "delta": data.delta,
                        "output_source": output_source,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                    }),
                })
            }
            (KernelStreamEventType::ToolCallStart, KernelStreamEventData::ToolCallStart(data)) => {
                Some(OrchestratorEvent {
                    event_type: "message.tool_call.started".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "call_id": data.call_id,
                        "name": data.tool_name,
                        "arguments": data.arguments,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                        "timing_ms": Value::Null,
                    }),
                })
            }
            (KernelStreamEventType::ToolResult, KernelStreamEventData::ToolResult(data)) => {
                let payload = serde_json::to_value(&data.content).unwrap_or_else(|_| json!([]));
                Some(OrchestratorEvent {
                    event_type: "message.tool_call.completed".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "call_id": data.call_id,
                        "tool": data.tool_name,
                        "content": [{
                            "type": "json",
                            "value": payload,
                        }],
                        "is_error": data.is_error,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                        "timing_ms": Value::Null,
                    }),
                })
            }
            (KernelStreamEventType::PromptWarning, KernelStreamEventData::PromptWarning(data)) => {
                Some(OrchestratorEvent {
                    event_type: "message.warning".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "warning": data.warning,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                    }),
                })
            }
            _ => None,
        };
        if mapped.is_some() {
            *item_index = (*item_index).saturating_add(1);
        }
        mapped
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
        let tool_context = self.tool_registry.build_context(
            format!("agent-{}", self.id),
            self.name.clone(),
            Some(Arc::new(self.kernel_config_snapshot())),
        );
        let tool_result = self
            .tool_registry
            .execute_with_context(tool_name, args, tool_context)
            .await?;

        let content = if tool_result.success {
            tool_result.output
        } else {
            format!(
                "Tool execution failed: {:?}",
                tool_result.error.unwrap_or("Unknown error".to_string())
            )
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
        let model = if agent_request.session.config.llm.model.is_empty() {
            self.model.clone()
        } else {
            agent_request.session.config.llm.model.clone()
        };
        let phase = agent_request.phase.clone();
        let planning_instruction = agent_request.planning_instruction.clone();
        let transfer_to = agent_request.transfer_to.clone();
        let kernel_agent = self.build_kernel_agent_for_request()?;
        let result = self
            .kernel_runtime
            .run_turn(
                kernel_agent,
                RunRequest {
                    session_id: agent_request.session.context.session_id.clone(),
                    invocation_id,
                    agent_id: self.id.clone(),
                    workspace_id: agent_request
                        .session
                        .context
                        .workspace_id
                        .clone()
                        .unwrap_or_default(),
                    input,
                    model,
                    phase,
                    planning_instruction,
                    transfer_to,
                    initial_state_bags: None,
                    kernel_config: Some(self.kernel_config.clone()),
                    stream_tx: None,
                },
            )
            .await?;
        let reasoning = Self::collect_reasoning(&result.output_items);
        Ok(OrchestratorExecution {
            output_text: result.output_text,
            output_items: result.output_items,
            reasoning,
            stop_reason: Self::map_stop_reason(result.stop_reason).to_string(),
            usage: result.usage,
            task_count: result.task_results.len() as u32,
            session_id: agent_request.session.context.session_id,
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let this = self.clone();
        tokio::spawn(async move {
            let agent_request = this.to_agent_request(request);
            let input = Self::extract_text_input(&agent_request.input);
            let request_id = format!("req_{}", Uuid::new_v4().simple());
            let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
            let model = if agent_request.session.config.llm.model.is_empty() {
                this.model.clone()
            } else {
                agent_request.session.config.llm.model.clone()
            };
            let phase = agent_request.phase.clone();
            let planning_instruction = agent_request.planning_instruction.clone();
            let transfer_to = agent_request.transfer_to.clone();
            let kernel_agent = match this.build_kernel_agent_for_request() {
                Ok(agent) => agent,
                Err(err) => {
                    let _ = tx
                        .send(OrchestratorEvent {
                            event_type: "response.error".to_string(),
                            data: json!({
                                "request_id": request_id,
                                "error": err.to_string(),
                            }),
                        })
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
                model: model.clone(),
                phase,
                planning_instruction,
                transfer_to,
                initial_state_bags: None,
                kernel_config: Some(this.kernel_config.clone()),
                stream_tx: Some(kernel_tx),
            };

            let _ = tx
                .send(OrchestratorEvent {
                    event_type: "message.started".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "model": model,
                        "usage": TalosUsage::default(),
                        "iteration": 0_u64,
                        "step_id": "start",
                    }),
                })
                .await;

            let run_handle =
                tokio::spawn(async move { runtime.run_turn(kernel_agent, kernel_input).await });

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
                        .send(OrchestratorEvent {
                            event_type: "message.completed".to_string(),
                            data: json!({
                                "stop_reason": result.stop_reason,
                                "request_id": request_id,
                                "usage": result.usage,
                                "total_output_items": item_index,
                                "iteration": item_index.saturating_add(1),
                                "step_id": "completed",
                                "task_count": result.task_results.len(),
                            }),
                        })
                        .await;
                }
                Ok(Err(err)) => {
                    let _ = tx
                        .send(OrchestratorEvent {
                            event_type: "message.error".to_string(),
                            data: json!({
                                "request_id": request_id,
                                "error": err.to_string(),
                            }),
                        })
                        .await;
                }
                Err(err) => {
                    let _ = tx
                        .send(OrchestratorEvent {
                            event_type: "message.error".to_string(),
                            data: json!({
                                "request_id": request_id,
                                "error": format!("kernel stream task join failed: {}", err),
                            }),
                        })
                        .await;
                }
            }
        });
        Ok(rx)
    }

    async fn close_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.finalize_session_memory(&session).await
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

    async fn run(&self, _ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
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
            stop_reason: "end_turn".to_string(),
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
                .send(OrchestratorEvent {
                    event_type: "message.started".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "model": "mock-model",
                        "usage": TalosUsage::default(),
                        "iteration": 0_u64,
                        "step_id": "start",
                    }),
                })
                .await;
            let _ = tx
                .send(OrchestratorEvent {
                    event_type: "message.output.delta".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "delta": execution.output_text,
                        "item_index": 0_u64,
                        "iteration": 1_u64,
                        "step_id": "step-1",
                    }),
                })
                .await;
            let _ = tx
                .send(OrchestratorEvent {
                    event_type: "message.completed".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "stop_reason": execution.stop_reason,
                        "usage": execution.usage,
                        "total_output_items": 1_u64,
                        "iteration": 2_u64,
                        "step_id": "completed",
                    }),
                })
                .await;
        });
        Ok(rx)
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use talos_agent_kernel::TurnPhase;
    use talos_config::AgentKernelConfig;
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Role, Usage};
    use talos_memory::{Memory, RecordHeader, TalosMessage, TalosMessageRole, TalosTurn};
    use talos_tools::{ReadTool, ToolRegistry, WebSearchTool};
    use talos_tools_policy::ToolPolicy;
    use talos_types::talos::{TalosProvider, TalosSourceType};
    use talos_types::{SessionContext, SessionState, TalosSession};
    use tempfile::TempDir;
    use tokio;

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

    fn mock_invocation_ctx() -> InvocationContext {
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
            execution_state: talos_agent_kernel::InvocationExecutionState::default(),
            session: talos_agent_kernel::InvocationSession {
                session_id: "s1".to_string(),
                ..talos_agent_kernel::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: Default::default(),
            limits: Default::default(),
            events: Vec::new(),
            stream_tx: None,
        }
    }

    fn build_test_session(session_id: &str, workspace_id: &str) -> TalosSession {
        TalosSession {
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
        agent.add_tool(Arc::new(ReadTool));

        // Generate system prompt
        let prompt = agent.generate_system_prompt();

        assert!(prompt.contains("Test Agent"));
        assert!(prompt.contains("An agent for testing"));
        assert!(prompt.contains("web_search"));
        assert!(prompt.contains("read"));
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
            messages: vec![
                TalosMessage {
                    header: RecordHeader {
                        id: "msg-user".to_string(),
                        session_id: session_id.to_string(),
                        turn_id: Some("turn-1".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::User,
                    text: "Remember that I prefer tea over coffee.".to_string(),
                },
                TalosMessage {
                    header: RecordHeader {
                        id: "msg-assistant".to_string(),
                        session_id: session_id.to_string(),
                        turn_id: Some("turn-1".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::Assistant,
                    text: "Noted. You prefer tea over coffee.".to_string(),
                },
            ],
            reasoning: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        };

        agent
            .memory_store
            .append_turn(&turn)
            .await
            .expect("session turn should be persisted");

        agent
            .close_session(session)
            .await
            .expect("session close should flush workspace memory");

        let file_path = _guard
            .path()
            .join("memory")
            .join(format!("{}.md", Utc::now().format("%Y-%m-%d")));
        let content = tokio::fs::read_to_string(file_path)
            .await
            .expect("workspace daily memory file should exist");
        assert!(content.contains("## Durable Memory"));
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
        let planner = agent
            .build_planner_for_request()
            .expect("planner should build");
        let ctx = mock_invocation_ctx();
        assert!(planner.build_planning_instruction(&ctx).is_some());
    }

    #[test]
    fn test_kernel_agent_kind_comes_from_binding_default() {
        let agent = build_test_agent();
        let kind = agent.resolve_kernel_agent_kind();
        assert_eq!(kind, KernelAgentKind::Llm);
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

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: "Test response".to_string(),
                model: "mock-model".to_string(),
                usage: Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                },
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
