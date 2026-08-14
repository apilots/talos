use crate::{
    ExecutionBranch, InvocationSession, KernelTranscriptService, KernelWarning, ResolvedAttachment,
    UserInput,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, TalosSessionEnvironment,
};
use talos_types::talos::{RunItem, TalosContentItem, TokenUsage, TurnExecutionMetrics};
use talos_types::{ModelInfo, ModelSelection};
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
pub enum FlowProfile {
    DirectAnswer,
    ModelThinking,
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
    pub output_items: Vec<RunItem>,
    #[serde(default)]
    pub usage: TokenUsage,
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
    ThinkingDelta,
    AssistantTextStarted,
    AssistantTextDelta,
    AssistantTextCompleted,
    AssistantTextCommitted,
    ExecutionSummary,
    TurnMetrics,
    ToolCallStart,
    ToolArgsDelta,
    ToolQueued,
    ToolExecuting,
    ToolProgressDelta,
    ToolResult,
    PromptWarning,
    UserActionRequest,
    ContextCompaction,
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
    ThinkingDelta(ThinkingDeltaStreamData),
    AssistantTextStarted(AssistantTextStartedStreamData),
    AssistantTextDelta(AssistantTextDeltaStreamData),
    AssistantTextCompleted(AssistantTextCompletedStreamData),
    AssistantTextCommitted(AssistantTextCommittedStreamData),
    ExecutionSummary(ExecutionSummaryStreamData),
    TurnMetrics(TurnMetricsStreamData),
    ToolCallStart(ToolCallStartStreamData),
    ToolArgsDelta(ToolArgsDeltaStreamData),
    ToolQueued(ToolQueuedStreamData),
    ToolExecuting(ToolExecutingStreamData),
    ToolProgressDelta(ToolProgressDeltaStreamData),
    ToolResult(ToolResultStreamData),
    PromptWarning(PromptWarningEventData),
    UserActionRequest(UserActionRequestStreamData),
    ContextCompaction(ContextCompactionStreamData),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct KernelStreamItemKey {
    pub round: Option<u32>,
    pub local_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantTextPhase {
    Unknown,
    Commentary,
    FinalAnswer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelModelCallMode {
    WorkingLoop,
    Finalization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantTextStartedStreamData {
    pub item_key: KernelStreamItemKey,
    pub phase: AssistantTextPhase,
    pub round: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantTextDeltaStreamData {
    pub item_key: KernelStreamItemKey,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantTextCompletedStreamData {
    pub item_key: KernelStreamItemKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantTextCommittedStreamData {
    pub item_key: KernelStreamItemKey,
    pub phase: AssistantTextPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnMetricsStreamData {
    pub metrics: TurnExecutionMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ToolStreamKey {
    pub round: u32,
    pub index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserActionRequestStreamData {
    pub action_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub params: talos_types::UserActionParams,
    pub sandbox_request_fingerprint: Option<String>,
}

/// Kernel stream payload for run-internal context compaction lifecycle updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCompactionStreamData {
    pub trigger: ContextCompactionTrigger,
    pub stage: ContextCompactionStage,
    #[serde(default)]
    pub outcome: Option<ContextCompactionOutcome>,
    #[serde(default)]
    pub metrics: Option<ContextCompactionMetrics>,
    #[serde(default)]
    pub message: Option<String>,
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
pub struct ToolArgsDeltaStreamData {
    pub stream_key: ToolStreamKey,
    pub call_id: Option<String>,
    pub tool_name: Option<String>,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolQueuedStreamData {
    pub stream_key: ToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutingStreamData {
    pub stream_key: ToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolProgressKind {
    Status,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolProgressDeltaStreamData {
    pub stream_key: ToolStreamKey,
    pub call_id: String,
    pub tool_name: String,
    pub message: String,
    pub progress_kind: ToolProgressKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultStreamData {
    pub call_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub content: Vec<TalosContentItem>,
    pub is_error: bool,
    #[serde(default)]
    pub semantic_events: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub envelope: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_turn_failure: Option<Value>,
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
    ModelOutputLimit,
    WaitingUserAction,
    TransferUnresolved,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InvocationContext {
    pub session_id: String,
    pub user_id: String,
    pub message_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub project_root: Option<PathBuf>,
    pub input: UserInput,
    #[serde(skip, default)]
    pub resolved_attachments: Vec<ResolvedAttachment>,
    pub model_selection: ModelSelection,
    pub selected_model_info: ModelInfo,
    #[serde(default)]
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub transfer_to: Option<String>,
    #[serde(default)]
    pub execution_state: InvocationExecutionState,
    #[serde(default)]
    pub session: InvocationSession,
    #[serde(default)]
    pub session_environment: Option<TalosSessionEnvironment>,
    pub state: HashMap<String, Value>,
    #[serde(default)]
    pub state_bags: ContextStateBags,
    #[serde(default)]
    pub limits: InvocationLimits,
    pub events: Vec<KernelEvent>,
    #[serde(skip, default)]
    pub transcript_service: Option<std::sync::Arc<dyn KernelTranscriptService>>,
    #[serde(skip, default)]
    pub session_artifact_store:
        Option<std::sync::Arc<dyn talos_store::session::SessionArtifactStore>>,
    #[serde(skip, default)]
    pub session_store: Option<std::sync::Arc<dyn talos_store::session::SessionStore>>,
    #[serde(skip)]
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

impl InvocationContext {
    pub fn selected_model_id(&self) -> &str {
        self.selected_model_info.model.id.as_str()
    }

    pub fn set_planning_response(&mut self, value: impl Into<String>) {
        self.execution_state.planning_response = Some(value.into());
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

    pub fn current_transcript_events(
        &self,
        current_invocation: bool,
        current_branch: bool,
        branch: ExecutionBranch,
    ) -> Vec<crate::KernelTranscriptEvent> {
        self.session.filter_events(
            current_invocation.then_some(self.invocation_id.as_str()),
            current_branch.then_some(branch),
        )
    }

    pub fn transcript_service(&self) -> TalosResult<std::sync::Arc<dyn KernelTranscriptService>> {
        self.transcript_service.clone().ok_or_else(|| {
            TalosError::ValidationError(
                "transcript_service is not configured in InvocationContext".to_string(),
            )
        })
    }

    pub fn session_artifact_store(
        &self,
    ) -> TalosResult<std::sync::Arc<dyn talos_store::session::SessionArtifactStore>> {
        self.session_artifact_store.clone().ok_or_else(|| {
            TalosError::ValidationError(
                "session_artifact_store is not configured in InvocationContext".to_string(),
            )
        })
    }

    pub fn session_store(
        &self,
    ) -> TalosResult<std::sync::Arc<dyn talos_store::session::SessionStore>> {
        self.session_store.clone().ok_or_else(|| {
            TalosError::ValidationError(
                "session_store is not configured in InvocationContext".to_string(),
            )
        })
    }
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
}

/// Runtime limits for invocation execution.
/// This is a simplified version that directly stores limit values
/// extracted from AgentKernelConfig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationLimits {
    pub tool_parallelism: usize,
    pub context_compaction_max_bytes: usize,
    pub dynamic_tool_timeout_ms: u64,
    pub dynamic_tool_timeout_behavior: talos_config::DynamicToolTimeoutBehavior,
}

impl Default for InvocationLimits {
    fn default() -> Self {
        let config = talos_config::AgentKernelConfig::default();
        Self {
            tool_parallelism: config.tool_parallelism,
            context_compaction_max_bytes: config.context_compaction_max_bytes,
            dynamic_tool_timeout_ms: config.tools.dynamic.response_timeout_ms,
            dynamic_tool_timeout_behavior: config.tools.dynamic.on_timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelExecutionSettings {
    pub planner_kind: PlannerKind,
    pub tool_execution_mode: ToolExecutionMode,
}

impl Default for KernelExecutionSettings {
    fn default() -> Self {
        Self {
            planner_kind: PlannerKind::BuiltIn,
            tool_execution_mode: ToolExecutionMode::Parallel,
        }
    }
}

impl TryFrom<&talos_config::AgentKernelConfig> for KernelExecutionSettings {
    type Error = talos_error::TalosError;

    fn try_from(config: &talos_config::AgentKernelConfig) -> TalosResult<Self> {
        let planner_kind = config.planner_mode;
        let tool_execution_mode = match config.tools.sandbox_policy.user_permission_mode {
            talos_config::ToolUserPermissionModeConfig::FullAccess
                if config.tool_parallelism > 1 =>
            {
                ToolExecutionMode::Parallel
            }
            talos_config::ToolUserPermissionModeConfig::AskForApproval
            | talos_config::ToolUserPermissionModeConfig::ApproveForMe
            | talos_config::ToolUserPermissionModeConfig::FullAccess => {
                ToolExecutionMode::Sequential
            }
        };

        Ok(Self {
            planner_kind,
            tool_execution_mode,
        })
    }
}

impl From<&talos_config::AgentKernelConfig> for InvocationLimits {
    fn from(config: &talos_config::AgentKernelConfig) -> Self {
        Self {
            tool_parallelism: config.tool_parallelism,
            context_compaction_max_bytes: config.context_compaction_max_bytes,
            dynamic_tool_timeout_ms: config.tools.dynamic.response_timeout_ms,
            dynamic_tool_timeout_behavior: config.tools.dynamic.on_timeout,
        }
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
