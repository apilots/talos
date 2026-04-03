use crate::{
    ExecutionBranch, InvocationPluginManager, InvocationSession, KernelWarning, SessionService,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{TalosContentItem, TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    #[default]
    Execution,
    Planning,
}

pub type PlannerKind = talos_config::PlannerMode;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    #[default]
    Single,
    Auto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlowProfile {
    DirectAnswer,
    ModelThinking,
    #[default]
    ReactiveLoop,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    Disabled,
    #[default]
    PlanningProcessor,
    ModelNative,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPath {
    DirectAnswer,
    #[default]
    ReactiveLoop,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Sequential,
    #[default]
    Parallel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum KernelExecutionKind {
    #[default]
    Single,
    Sequential,
    Parallel,
    Loop,
}

impl KernelExecutionKind {
    pub fn from_mode(value: talos_config::ExecutionMode) -> Self {
        match value {
            talos_config::ExecutionMode::Single => Self::Single,
            talos_config::ExecutionMode::Sequential => Self::Sequential,
            talos_config::ExecutionMode::Parallel => Self::Parallel,
            talos_config::ExecutionMode::Loop => Self::Loop,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Success,
    Partial,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: String,
    pub kind: TaskKind,
    pub goal: String,
    pub status: TaskStatus,
    pub output: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    #[serde(default)]
    pub usage: TalosUsage,
    #[serde(default)]
    pub warnings: Vec<KernelWarning>,
    pub tool_traces: Vec<Value>,
    pub state_delta: HashMap<String, Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelEventType {
    UserInput,
    PlanCreated,
    PromptWarning,
    TaskCompleted,
    Transfer,
    FinalResponse,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelEventLabel {
    UserInputReceived,
    Iteration,
    TaskCompleted,
    TransferRequested,
    FinalResponse,
    PromptWarning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelEvent {
    pub event_type: KernelEventType,
    pub label: KernelEventLabel,
    pub data: KernelEventData,
    pub timestamp: i64,
}

impl KernelEvent {
    pub fn new(
        event_type: KernelEventType,
        label: KernelEventLabel,
        data: KernelEventData,
    ) -> Self {
        Self {
            event_type,
            label,
            data,
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KernelEventData {
    #[default]
    None,
    UserInput(UserInputEventData),
    Iteration(IterationEventData),
    TaskCompleted(TaskCompletedEventData),
    TransferRequested(TransferRequestedEventData),
    FinalResponse(FinalResponseEventData),
    PromptWarning(PromptWarningEventData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInputEventData {
    pub session_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationEventData {
    pub iteration: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletedEventData {
    pub task_id: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferRequestedEventData {
    pub target_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalResponseEventData {
    pub iterations: u32,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptWarningEventData {
    pub warning: KernelWarning,
    pub agent_id: String,
    pub session_id: String,
    pub phase: TurnPhase,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelStreamEventType {
    OutputDelta,
    ThinkingDelta,
    ExecutionSummary,
    ToolCallStart,
    ToolResult,
    PromptWarning,
    ConfirmationRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelStreamEvent {
    pub event_type: KernelStreamEventType,
    pub data: KernelStreamEventData,
    pub timestamp: i64,
}

impl KernelStreamEvent {
    pub fn new(event_type: KernelStreamEventType, data: KernelStreamEventData) -> Self {
        Self {
            event_type,
            data,
            timestamp: Utc::now().timestamp_millis(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KernelStreamEventData {
    OutputDelta(OutputDeltaStreamData),
    ThinkingDelta(ThinkingDeltaStreamData),
    ExecutionSummary(ExecutionSummaryStreamData),
    ToolCallStart(ToolCallStartStreamData),
    ToolResult(ToolResultStreamData),
    PromptWarning(PromptWarningEventData),
    ConfirmationRequest(ConfirmationRequestStreamData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequestStreamData {
    pub call_id: String,
    pub tool_name: String,
    pub prompt: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDeltaStreamData {
    pub delta: String,
    pub round: Option<u32>,
    pub source: Option<KernelOutputDeltaSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingDeltaStreamData {
    pub delta: String,
    pub source: KernelThinkingSource,
    pub stage: KernelThinkingStage,
    pub round: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummaryStreamData {
    pub text: String,
    pub round: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStartStreamData {
    pub index: usize,
    pub call_id: Option<String>,
    pub tool_name: Option<String>,
    pub arguments: Value,
    pub round: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultStreamData {
    pub call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub content: Vec<TalosContentItem>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelOutputDeltaSource {
    Provider,
    KernelFallback,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelThinkingSource {
    Provider,
    Framework,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelThinkingStage {
    ModelReasoning,
    FrameworkPlan,
    FrameworkAct,
    FrameworkObserve,
    FrameworkReflect,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolRoundLimit,
    TransferUnresolved,
    BeforeModelCallbackStopped,
    AfterModelCallbackStopped,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InvocationContext {
    pub session_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub input: String,
    pub model: String,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
    #[serde(default)]
    pub execution_state: InvocationExecutionState,
    #[serde(default)]
    pub session: InvocationSession,
    pub state: HashMap<String, Value>,
    #[serde(default)]
    pub state_bags: ContextStateBags,
    #[serde(default)]
    pub limits: InvocationLimits,
    pub events: Vec<KernelEvent>,
    #[serde(skip, default = "crate::default_plugin_manager")]
    pub plugin_manager: std::sync::Arc<InvocationPluginManager>,
    #[serde(skip, default)]
    pub session_service: Option<std::sync::Arc<dyn SessionService>>,
    #[serde(skip)]
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

impl InvocationContext {
    pub fn set_planning_response(&mut self, value: impl Into<String>) {
        self.execution_state.planning_response = Some(value.into());
    }

    pub fn set_plan_react_has_final_answer(&mut self, value: bool) {
        self.execution_state.plan_react_has_final_answer = value;
    }

    pub fn mark_transfer_consumed(&mut self) {
        self.execution_state.transfer_consumed = true;
    }

    pub fn transfer_consumed(&self) -> bool {
        self.execution_state.transfer_consumed
    }

    pub fn set_session_state(&mut self, key: impl Into<String>, value: Value) {
        self.state_bags.session_state.insert(key.into(), value);
    }

    pub fn set_request_state(&mut self, key: impl Into<String>, value: Value) {
        self.state_bags.request_state.insert(key.into(), value);
    }

    pub fn set_user_state(&mut self, _key: impl Into<String>, _value: Value) -> TalosResult<()> {
        Err(TalosError::ValidationError(
            "user_state is read-only by default in InvocationContext".to_string(),
        ))
    }

    pub fn set_workspace_state(
        &mut self,
        _key: impl Into<String>,
        _value: Value,
    ) -> TalosResult<()> {
        Err(TalosError::ValidationError(
            "workspace_state is read-only by default in InvocationContext".to_string(),
        ))
    }

    pub fn current_session_events(
        &self,
        current_invocation: bool,
        current_branch: bool,
        branch: ExecutionBranch,
    ) -> Vec<crate::SessionEvent> {
        self.session.filter_events(
            current_invocation.then_some(self.invocation_id.as_str()),
            current_branch.then_some(branch),
        )
    }

    pub fn session_service(&self) -> TalosResult<std::sync::Arc<dyn SessionService>> {
        self.session_service.clone().ok_or_else(|| {
            TalosError::ValidationError(
                "session_service is not configured in InvocationContext".to_string(),
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunResult {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TalosUsage,
    #[serde(default)]
    pub warnings: Vec<KernelWarning>,
    pub task_results: Vec<TaskResult>,
    pub state_delta: HashMap<String, Value>,
    pub continue_loop: bool,
    pub requested_transfer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextStateBags {
    #[serde(default)]
    pub session_state: HashMap<String, Value>,
    #[serde(default)]
    pub user_state: HashMap<String, Value>,
    #[serde(default)]
    pub workspace_state: HashMap<String, Value>,
    #[serde(default)]
    pub request_state: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct InvocationExecutionState {
    pub planning_response: Option<String>,
    #[serde(default)]
    pub transfer_consumed: bool,
    /// Set by PlanningResponseProcessor when PlanReAct mode detects a FINAL_ANSWER tag.
    #[serde(default)]
    pub plan_react_has_final_answer: bool,
}

/// Runtime limits for invocation execution.
/// This is a simplified version that directly stores limit values
/// extracted from AgentKernelConfig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationLimits {
    pub max_iters: u32,
    pub max_task_per_turn: u32,
    pub tool_parallelism: usize,
    pub max_llm_calls: u32,
    pub context_compaction_max_bytes: usize,
    pub dynamic_tool_timeout_ms: u64,
    pub dynamic_tool_timeout_behavior: talos_config::DynamicToolTimeoutBehavior,
}

impl Default for InvocationLimits {
    fn default() -> Self {
        let config = talos_config::AgentKernelConfig::default();
        Self {
            max_iters: config.max_iters,
            max_task_per_turn: config.max_task_per_turn as u32,
            tool_parallelism: config.tool_parallelism,
            max_llm_calls: config.max_llm_calls,
            context_compaction_max_bytes: config.context_compaction_max_bytes,
            dynamic_tool_timeout_ms: config.tools.dynamic.response_timeout_ms,
            dynamic_tool_timeout_behavior: config.tools.dynamic.on_timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelExecutionSettings {
    pub planner_kind: PlannerKind,
    pub execution_kind: KernelExecutionKind,
    pub flow_kind: FlowKind,
    pub thinking_mode: ThinkingMode,
    pub default_path: ExecutionPath,
    pub tool_execution_mode: ToolExecutionMode,
}

impl Default for KernelExecutionSettings {
    fn default() -> Self {
        Self {
            planner_kind: PlannerKind::BuiltIn,
            execution_kind: KernelExecutionKind::Single,
            flow_kind: FlowKind::Single,
            thinking_mode: ThinkingMode::PlanningProcessor,
            default_path: ExecutionPath::ReactiveLoop,
            tool_execution_mode: ToolExecutionMode::Parallel,
        }
    }
}

impl TryFrom<&talos_config::AgentKernelConfig> for KernelExecutionSettings {
    type Error = talos_error::TalosError;

    fn try_from(config: &talos_config::AgentKernelConfig) -> TalosResult<Self> {
        let planner_kind = config.planner_mode;
        let execution_kind = KernelExecutionKind::from_mode(config.execution_mode);
        let flow_kind = match execution_kind {
            KernelExecutionKind::Single => FlowKind::Single,
            _ => FlowKind::Auto,
        };
        let thinking_mode = match planner_kind {
            PlannerKind::BuiltIn => ThinkingMode::ModelNative,
            PlannerKind::PlanReAct => ThinkingMode::PlanningProcessor,
        };
        let default_path = match planner_kind {
            PlannerKind::BuiltIn => ExecutionPath::DirectAnswer,
            PlannerKind::PlanReAct => ExecutionPath::ReactiveLoop,
        };
        let tool_execution_mode = if config.tool_parallelism > 1 {
            ToolExecutionMode::Parallel
        } else {
            ToolExecutionMode::Sequential
        };

        Ok(Self {
            planner_kind,
            execution_kind,
            flow_kind,
            thinking_mode,
            default_path,
            tool_execution_mode,
        })
    }
}

impl From<&talos_config::AgentKernelConfig> for InvocationLimits {
    fn from(config: &talos_config::AgentKernelConfig) -> Self {
        Self {
            max_iters: config.max_iters,
            max_task_per_turn: config.max_task_per_turn as u32,
            tool_parallelism: config.tool_parallelism,
            max_llm_calls: config.max_llm_calls,
            context_compaction_max_bytes: config.context_compaction_max_bytes,
            dynamic_tool_timeout_ms: config.tools.dynamic.response_timeout_ms,
            dynamic_tool_timeout_behavior: config.tools.dynamic.on_timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub session_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub input: String,
    pub model: String,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
    pub initial_state_bags: Option<ContextStateBags>,
    pub kernel_config: Option<talos_config::AgentKernelConfig>,
    #[serde(skip)]
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub output_text: String,
    #[serde(default)]
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    #[serde(default)]
    pub usage: TalosUsage,
    #[serde(default)]
    pub warnings: Vec<KernelWarning>,
    pub model: String,
    pub task_results: Vec<TaskResult>,
    pub transfer_to: Option<String>,
    pub iterations: u32,
    pub events: Vec<KernelEvent>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_invocation_context_state_write_scopes() {
        let mut ctx = InvocationContext {
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
            plugin_manager: crate::default_plugin_manager(),
            session_service: None,
            stream_tx: None,
        };

        ctx.set_session_state("k1", json!("v1"));
        ctx.set_request_state("k2", json!("v2"));
        ctx.set_planning_response("ok");
        ctx.mark_transfer_consumed();

        assert_eq!(ctx.state_bags.session_state.get("k1"), Some(&json!("v1")));
        assert_eq!(ctx.state_bags.request_state.get("k2"), Some(&json!("v2")));
        assert_eq!(ctx.execution_state.planning_response.as_deref(), Some("ok"));
        assert!(ctx.transfer_consumed());
        assert!(ctx.set_user_state("k3", json!("v3")).is_err());
        assert!(ctx.set_workspace_state("k4", json!("v4")).is_err());
    }

    #[test]
    fn test_kernel_execution_settings_follow_planner_defaults() {
        let built_in = talos_config::AgentKernelConfig {
            planner_mode: talos_config::PlannerMode::BuiltIn,
            ..talos_config::AgentKernelConfig::default()
        };
        let built_in_settings = KernelExecutionSettings::try_from(&built_in).unwrap();
        assert_eq!(built_in_settings.thinking_mode, ThinkingMode::ModelNative);
        assert_eq!(built_in_settings.default_path, ExecutionPath::DirectAnswer);

        let plan_react = talos_config::AgentKernelConfig {
            planner_mode: talos_config::PlannerMode::PlanReAct,
            ..talos_config::AgentKernelConfig::default()
        };
        let plan_react_settings = KernelExecutionSettings::try_from(&plan_react).unwrap();
        assert_eq!(
            plan_react_settings.thinking_mode,
            ThinkingMode::PlanningProcessor
        );
        assert_eq!(
            plan_react_settings.default_path,
            ExecutionPath::ReactiveLoop
        );
    }
}
