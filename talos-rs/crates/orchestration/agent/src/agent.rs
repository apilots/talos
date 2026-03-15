use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    Agent, AgentOrchestrator, AgentResponse, AgentStream, OrchestratorEvent, OrchestratorExecution,
    OrchestratorRequest,
};
use serde_json::{json, Value};
use talos_agent_kernel::{
    Agent as KernelAgent, AgentRunResult, BuiltInPlanner, InvocationContext, KernelInput,
    KernelRuntime, KernelStreamEvent, LlmKernelAgent, LoopKernelAgent,
    ParallelKernelAgent, PlanReActPlanner, Planner, RoutingKernelAgent, SequentialKernelAgent,
    SessionHandle, TaskKind, TaskResult, TaskStatus,
};
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, FunctionCall, LlmProvider};
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::canonical::{CanonicalOutputItem, CanonicalUsage};

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

    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "built_in" | "builtin" => Some(Self::BuiltIn),
            "plan_react" | "planreact" => Some(Self::PlanReAct),
            _ => None,
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

    fn supports_kind(&self, kind: &str) -> bool {
        self.kind_map.contains_key(&kind.to_ascii_lowercase())
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
    fn normalized_kernel_config(mut kernel_config: AgentKernelConfig) -> AgentKernelConfig {
        if PlannerMode::parse(&kernel_config.planner_mode).is_none() {
            kernel_config.planner_mode = PlannerMode::BuiltIn.as_str().to_string();
        }
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
        kernel_config.memory.context_target_messages =
            kernel_config.memory.context_target_messages.max(1);
        kernel_config.memory.summary_recent_count =
            kernel_config.memory.summary_recent_count.max(1);
        kernel_config.memory.summary_max_chars = kernel_config.memory.summary_max_chars.max(128);
        kernel_config.memory.compaction_threshold_bytes =
            kernel_config.memory.compaction_threshold_bytes.max(1024);
        kernel_config.memory.long_term_min_importance = kernel_config
            .memory
            .long_term_min_importance
            .clamp(0.0, 1.0);
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

    fn default_planner_mode(&self) -> PlannerMode {
        PlannerMode::parse(&self.kernel_config.planner_mode).unwrap_or(PlannerMode::BuiltIn)
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

    pub fn set_default_planner_mode(&mut self, planner_mode: &str) {
        if let Some(mode) = PlannerMode::parse(planner_mode.trim()) {
            self.kernel_config.planner_mode = mode.as_str().to_string();
        }
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
            HashMap::new(),
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

    fn infer_reasoning_source(text: &str) -> &'static str {
        if text.trim_start().starts_with("Kernel summary:") {
            "kernel"
        } else {
            "provider"
        }
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

    fn merged_kernel_metadata(&self, metadata: Option<Value>) -> Option<Value> {
        let mut data = match metadata {
            Some(Value::Object(map)) => map,
            Some(other) => {
                let mut map = serde_json::Map::new();
                map.insert("request_metadata".to_string(), other);
                map
            }
            None => serde_json::Map::new(),
        };
        let kernel = &self.kernel_config;
        Self::upsert_u64(
            &mut data,
            "max_tool_rounds",
            kernel.max_tool_rounds as u64,
            kernel.max_tool_rounds as u64,
        );
        Self::upsert_u64(
            &mut data,
            "tool_output_max_bytes",
            kernel.tool_result_max_bytes as u64,
            kernel.tool_result_max_bytes as u64,
        );
        Self::upsert_u64(
            &mut data,
            "context_compaction_max_bytes",
            kernel.context_compaction_max_bytes as u64,
            kernel.context_compaction_max_bytes as u64,
        );
        Self::upsert_u64(
            &mut data,
            "prompt_max_static_tokens",
            kernel.prompt.max_static_tokens as u64,
            kernel.prompt.max_static_tokens as u64,
        );
        Self::upsert_u64(
            &mut data,
            "prompt_max_dynamic_tokens",
            kernel.prompt.max_dynamic_tokens as u64,
            kernel.prompt.max_dynamic_tokens as u64,
        );
        Self::upsert_bool(
            &mut data,
            "prompt_include_workspace_summary",
            kernel.prompt.include_workspace_summary,
        );
        Self::upsert_bool(
            &mut data,
            "prompt_include_recent_messages",
            kernel.prompt.include_recent_messages,
        );
        Self::upsert_u64(
            &mut data,
            "memory_context_target_messages",
            kernel.memory.context_target_messages as u64,
            kernel.memory.context_target_messages as u64,
        );
        Self::upsert_bool(
            &mut data,
            "memory_enable_summary",
            kernel.memory.enable_summary,
        );
        Self::upsert_u64(
            &mut data,
            "memory_summary_recent_count",
            kernel.memory.summary_recent_count as u64,
            kernel.memory.summary_recent_count as u64,
        );
        Self::upsert_u64(
            &mut data,
            "memory_summary_max_chars",
            kernel.memory.summary_max_chars as u64,
            kernel.memory.summary_max_chars as u64,
        );
        Self::upsert_u64(
            &mut data,
            "memory_compaction_threshold_bytes",
            kernel.memory.compaction_threshold_bytes as u64,
            kernel.memory.compaction_threshold_bytes as u64,
        );
        Self::upsert_bool(
            &mut data,
            "memory_long_term_fact_extraction_enabled",
            kernel.memory.long_term_fact_extraction_enabled,
        );
        Self::upsert_f64(
            &mut data,
            "memory_long_term_min_importance",
            kernel.memory.long_term_min_importance as f64,
        );

        let default_planner_mode = self.default_planner_mode();
        if !kernel.allow_request_planner_override {
            data.insert(
                "planner_mode".to_string(),
                json!(default_planner_mode.as_str()),
            );
        } else {
            let normalized = data
                .get("planner_mode")
                .and_then(Value::as_str)
                .and_then(PlannerMode::parse)
                .unwrap_or(default_planner_mode)
                .as_str()
                .to_string();
            data.insert("planner_mode".to_string(), json!(normalized));
        }

        let resolved_agent_kind =
            Self::metadata_str(Some(&Value::Object(data.clone())), "kernel_agent_kind")
                .unwrap_or_else(|| self.kernel_agent_registry.default_kind().to_string())
                .to_ascii_lowercase();
        data.insert("kernel_agent_kind".to_string(), json!(resolved_agent_kind));

        Some(Value::Object(data))
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

    fn resolve_planner_mode(&self, metadata: Option<&Value>) -> PlannerMode {
        let default_mode = self.default_planner_mode();
        if !self.kernel_config.allow_request_planner_override {
            return default_mode;
        }
        Self::metadata_str(metadata, "planner_mode")
            .and_then(|value| PlannerMode::parse(&value))
            .unwrap_or(default_mode)
    }

    fn build_planner_for_request(&self, metadata: Option<&Value>) -> Arc<dyn Planner> {
        if let Some(planner) = self.planner_override.as_ref() {
            return planner.clone();
        }
        match self.resolve_planner_mode(metadata) {
            PlannerMode::BuiltIn => Arc::new(BuiltInPlanner::new()),
            PlannerMode::PlanReAct => Arc::new(PlanReActPlanner::new()),
        }
    }

    fn resolve_kernel_agent_kind(&self, metadata: Option<&Value>) -> KernelAgentKind {
        let requested = Self::metadata_str(metadata, "kernel_agent_kind");
        if let Some(kind) = requested.as_deref() {
            if !self.kernel_agent_registry.supports_kind(kind) {
                tracing::warn!(
                    requested_kind = %kind,
                    fallback_kind = %self.kernel_agent_registry.default_kind(),
                    "unknown kernel_agent_kind, fallback to default"
                );
            }
        }
        self.kernel_agent_registry.resolve(requested.as_deref())
    }

    fn metadata_u64(metadata: Option<&Value>, key: &str) -> Option<u64> {
        metadata
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_u64())
    }

    fn metadata_str(metadata: Option<&Value>, key: &str) -> Option<String> {
        metadata
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    }

    fn upsert_u64(data: &mut serde_json::Map<String, Value>, key: &str, default: u64, cap: u64) {
        let bounded = data
            .get(key)
            .and_then(Value::as_u64)
            .map(|value| value.clamp(1, cap.max(1)))
            .unwrap_or(default.max(1));
        data.insert(key.to_string(), json!(bounded));
    }

    fn upsert_bool(data: &mut serde_json::Map<String, Value>, key: &str, default: bool) {
        let value = data.get(key).and_then(Value::as_bool).unwrap_or(default);
        data.insert(key.to_string(), json!(value));
    }

    fn upsert_f64(data: &mut serde_json::Map<String, Value>, key: &str, default: f64) {
        let value = data.get(key).and_then(Value::as_f64).unwrap_or(default);
        data.insert(key.to_string(), json!(value));
    }

    fn resolve_kernel_mode(metadata: Option<&Value>) -> KernelExecutionMode {
        let mode = Self::metadata_str(metadata, "kernel_mode")
            .unwrap_or_else(|| "single".to_string())
            .to_ascii_lowercase();
        match mode.as_str() {
            "sequential" => KernelExecutionMode::Sequential,
            "parallel" => KernelExecutionMode::Parallel,
            "loop" => KernelExecutionMode::Loop,
            "a2a" | "route" | "routing" => KernelExecutionMode::A2A,
            _ => KernelExecutionMode::Single,
        }
    }

    fn build_kernel_agent_for_request(&self, metadata: Option<&Value>) -> Arc<dyn KernelAgent> {
        let planner = self.build_planner_for_request(metadata);
        let agent_kind = self.resolve_kernel_agent_kind(metadata);
        let mode = Self::resolve_kernel_mode(metadata);
        let child_count = Self::metadata_u64(metadata, "kernel_children")
            .unwrap_or(2)
            .clamp(1, 4) as usize;
        let loop_iters = Self::metadata_u64(metadata, "kernel_loop_iters")
            .unwrap_or(3)
            .clamp(1, 16) as u32;

        match mode {
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
        }
    }

    fn map_kernel_stream_event(
        event: KernelStreamEvent,
        request_id: &str,
        item_index: &mut u64,
    ) -> Option<OrchestratorEvent> {
        let current_index = *item_index;
        let iteration = current_index.saturating_add(1);
        let step_id = format!("step-{}", iteration);
        let mapped = match event.event_type.as_str() {
            "reasoning_delta" => {
                let delta = event
                    .data
                    .get("delta")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let source = event
                    .data
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| Self::infer_reasoning_source(&delta).to_string());
                Some(OrchestratorEvent {
                    event_type: "message.reasoning.delta".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "delta": delta,
                        "reasoning_source": source,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                    }),
                })
            }
            "output_delta" => {
                let delta = event
                    .data
                    .get("delta")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                Some(OrchestratorEvent {
                    event_type: "message.output.delta".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "delta": delta,
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                    }),
                })
            }
            "tool_call_start" => Some(OrchestratorEvent {
                event_type: "message.tool_call.started".to_string(),
                data: json!({
                    "request_id": request_id,
                    "call_id": event.data.get("id").cloned().unwrap_or(Value::Null),
                    "name": event.data.get("name").cloned().unwrap_or(Value::Null),
                    "arguments": event.data.get("arguments").cloned().unwrap_or(json!({})),
                    "item_index": current_index,
                    "iteration": iteration,
                    "step_id": step_id,
                    "timing_ms": Value::Null,
                }),
            }),
            "tool_result" => {
                let payload = event
                    .data
                    .get("payload")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                Some(OrchestratorEvent {
                    event_type: "message.tool_call.completed".to_string(),
                    data: json!({
                        "request_id": request_id,
                        "call_id": event.data.get("call_id").cloned().unwrap_or(Value::Null),
                        "tool": event.data.get("tool").cloned().unwrap_or(Value::Null),
                        "content": [{
                            "type": "json",
                            "value": payload,
                        }],
                        "is_error": event.data.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false),
                        "item_index": current_index,
                        "iteration": iteration,
                        "step_id": step_id,
                        "timing_ms": Value::Null,
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

    async fn handle_message(&self, content: &str) -> TalosResult<String> {
        self.handle_message_with_model(content, None).await
    }

    async fn handle_message_with_model(
        &self,
        content: &str,
        model: Option<&str>,
    ) -> TalosResult<String> {
        let execution = <Self as AgentOrchestrator>::run(
            self,
            OrchestratorRequest {
                agent_id: self.id.clone(),
                session_id: "embedded-session".to_string(),
                invocation_id: Some(format!("inv-{}", Utc::now().timestamp_millis())),
                workspace_id: "default".to_string(),
                input: content.to_string(),
                model: model.map(ToString::to_string),
                metadata: None,
            },
        )
        .await?;
        Ok(execution.output_text)
    }

    async fn stream_response(&self, content: &str) -> TalosResult<mpsc::Receiver<AgentStream>> {
        let mut kernel_events = <Self as AgentOrchestrator>::run_stream(
            self,
            OrchestratorRequest {
                agent_id: self.id.clone(),
                session_id: "embedded-session".to_string(),
                invocation_id: Some(format!("inv-{}", Utc::now().timestamp_millis())),
                workspace_id: "default".to_string(),
                input: content.to_string(),
                model: None,
                metadata: None,
            },
        )
        .await?;
        let (tx, rx) = mpsc::channel(100);
        tokio::spawn(async move {
            while let Some(event) = kernel_events.recv().await {
                let _ = tx
                    .send(AgentStream {
                        event_type: event.event_type,
                        data: event.data,
                    })
                    .await;
            }
        });

        Ok(rx)
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
            HashMap::from([(
                "tool_output_max_bytes".to_string(),
                json!(self.kernel_config.tool_result_max_bytes),
            )]),
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
            content,
            tool_calls: vec![], // Tool calls from within a tool execution context would go here
            metadata: HashMap::new(),
        })
    }
}

#[async_trait]
impl AgentOrchestrator for AgentImpl {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        let invocation_id = request
            .invocation_id
            .unwrap_or_else(|| format!("inv-{}", Utc::now().timestamp_millis()));
        let model = request.model.unwrap_or_else(|| self.model.clone());
        let metadata = self.merged_kernel_metadata(request.metadata);
        let session_handle = Some(SessionHandle {
            selected_model: Some(model.clone()),
            session_metadata: metadata.clone(),
            trace_id: metadata
                .as_ref()
                .and_then(|value| value.get("trace_id"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            channel_id: metadata
                .as_ref()
                .and_then(|value| value.get("channel_id"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            user_id: metadata
                .as_ref()
                .and_then(|value| value.get("user_id"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            recent_messages: Vec::new(),
            global_context: HashMap::new(),
        });
        let kernel_agent = self.build_kernel_agent_for_request(metadata.as_ref());
        let result = self
            .kernel_runtime
            .run_turn(
                kernel_agent,
                KernelInput {
                    session_id: request.session_id,
                    invocation_id: Some(invocation_id),
                    agent_id: request.agent_id,
                    workspace_id: request.workspace_id,
                    input: request.input,
                    model,
                    metadata,
                    session_handle,
                    initial_state_bags: None,
                    kernel_config: Some(self.kernel_config.clone()),
                    stream_tx: None,
                },
            )
            .await?;
        Ok(OrchestratorExecution {
            output_text: result.output_text,
            output_items: result.output_items,
            stop_reason: result.stop_reason,
            usage: result.usage,
            task_count: result.task_results.len() as u32,
            metadata: result.metadata,
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let this = self.clone();
        tokio::spawn(async move {
            let request_id = format!("req_{}", Uuid::new_v4().simple());
            let invocation_id = request
                .invocation_id
                .unwrap_or_else(|| format!("inv-{}", Utc::now().timestamp_millis()));
            let model = request.model.unwrap_or_else(|| this.model.clone());
            let metadata = this.merged_kernel_metadata(request.metadata);
            let session_handle = Some(SessionHandle {
                selected_model: Some(model.clone()),
                session_metadata: metadata.clone(),
                trace_id: metadata
                    .as_ref()
                    .and_then(|value| value.get("trace_id"))
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string()),
                channel_id: metadata
                    .as_ref()
                    .and_then(|value| value.get("channel_id"))
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string()),
                user_id: metadata
                    .as_ref()
                    .and_then(|value| value.get("user_id"))
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string()),
                recent_messages: Vec::new(),
                global_context: HashMap::new(),
            });
            let kernel_agent = this.build_kernel_agent_for_request(metadata.as_ref());
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let runtime = this.kernel_runtime.clone();
            let kernel_input = KernelInput {
                session_id: request.session_id,
                invocation_id: Some(invocation_id),
                agent_id: request.agent_id,
                workspace_id: request.workspace_id,
                input: request.input,
                model: model.clone(),
                metadata,
                session_handle,
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
                        "usage": CanonicalUsage::default(),
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

    async fn handle_message(&self, content: &str) -> TalosResult<String> {
        Ok(format!("Mock response to: {}", content))
    }

    async fn stream_response(&self, _content: &str) -> TalosResult<mpsc::Receiver<AgentStream>> {
        let (tx, rx) = mpsc::channel(1);
        drop(tx); // Close the sender immediately for mock
        Ok(rx)
    }

    async fn execute_tool(
        &self,
        _tool_name: &str,
        _args: serde_json::Value,
    ) -> TalosResult<AgentResponse> {
        Ok(AgentResponse {
            content: "Mock tool execution".to_string(),
            tool_calls: vec![],
            metadata: std::collections::HashMap::new(),
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
            output_items: vec![CanonicalOutputItem::OutputText {
                text: "Mock response".to_string(),
            }],
            stop_reason: "end_turn".to_string(),
            usage: CanonicalUsage::default(),
            task_results: vec![TaskResult {
                task_id: "mock-task".to_string(),
                kind: TaskKind::General,
                goal: "mock".to_string(),
                status: TaskStatus::Success,
                output: "Mock response".to_string(),
                output_items: vec![CanonicalOutputItem::OutputText {
                    text: "Mock response".to_string(),
                }],
                tool_traces: Vec::new(),
                state_delta: HashMap::new(),
                error: None,
            }],
            state_delta: HashMap::new(),
            continue_loop: false,
            requested_transfer: None,
            metadata: HashMap::new(),
        })
    }
}

#[async_trait]
impl AgentOrchestrator for MockAgent {
    fn id(&self) -> &str {
        "mock"
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        let output = self
            .handle_message_with_model(&request.input, request.model.as_deref())
            .await?;
        Ok(OrchestratorExecution {
            output_text: output,
            output_items: vec![CanonicalOutputItem::OutputText {
                text: "Mock response".to_string(),
            }],
            stop_reason: "end_turn".to_string(),
            usage: CanonicalUsage::default(),
            task_count: 1,
            metadata: HashMap::new(),
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
                        "usage": CanonicalUsage::default(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Role, Usage};
    use talos_tools::{ReadTool, ToolRegistry, WebSearchTool};
    use talos_tools_policy::ToolPolicy;
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

    fn mock_invocation_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            metadata: None,
            state: HashMap::new(),
            state_bags: Default::default(),
            session_handle: Some(SessionHandle::default()),
            limits: Default::default(),
            events: Vec::new(),
            stream_tx: None,
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
        assert!(response.content.contains("Search results for 'test'"));
    }

    #[test]
    fn test_derive_exec_command() {
        assert_eq!(AgentImpl::derive_exec_command("run echo ok"), "echo ok");
        assert_eq!(AgentImpl::derive_exec_command("execute ls"), "ls");
        assert_eq!(AgentImpl::derive_exec_command("exec pwd"), "pwd");
        assert_eq!(AgentImpl::derive_exec_command("echo plain"), "echo plain");
    }

    #[test]
    fn test_merged_kernel_metadata_respects_planner_override_policy_and_budget_caps() {
        let mut agent = build_test_agent();
        agent.set_default_planner_mode("plan_react");
        agent.set_allow_request_planner_override(false);
        agent.set_prompt_budget(1200, 3200);

        let merged = agent
            .merged_kernel_metadata(Some(json!({
                "planner_mode": "built_in",
                "prompt_max_static_tokens": 99999,
                "prompt_max_dynamic_tokens": 88888
            })))
            .expect("merged metadata should exist");

        assert_eq!(
            merged
                .get("planner_mode")
                .and_then(|v| v.as_str())
                .expect("planner_mode should exist"),
            "plan_react"
        );
        assert_eq!(
            merged
                .get("prompt_max_static_tokens")
                .and_then(|v| v.as_u64())
                .expect("prompt_max_static_tokens should exist"),
            1200
        );
        assert_eq!(
            merged
                .get("prompt_max_dynamic_tokens")
                .and_then(|v| v.as_u64())
                .expect("prompt_max_dynamic_tokens should exist"),
            3200
        );
    }

    #[test]
    fn test_planner_mode_override_enabled_and_invalid_value_fallback() {
        let mut agent = build_test_agent();
        agent.set_default_planner_mode("built_in");
        agent.set_allow_request_planner_override(true);

        let plan_react_meta = json!({ "planner_mode": "plan_react" });
        let planner = agent.build_planner_for_request(Some(&plan_react_meta));
        let ctx = mock_invocation_ctx();
        assert!(planner.build_planning_instruction(&ctx).is_some());

        let invalid_meta = json!({ "planner_mode": "invalid_mode" });
        let planner = agent.build_planner_for_request(Some(&invalid_meta));
        assert!(planner.build_planning_instruction(&ctx).is_none());
    }

    #[test]
    fn test_unknown_kernel_agent_kind_fallbacks_to_llm() {
        let agent = build_test_agent();
        let metadata = json!({ "kernel_agent_kind": "unknown_kind" });
        let kind = agent.resolve_kernel_agent_kind(Some(&metadata));
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
