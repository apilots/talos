use crate::{
    InvocationSession, KernelMemoryProgressStore, KernelSessionArtifactStore,
    KernelTranscriptService, KernelWarning, ResolvedAttachment, UserInput,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use talos_error::TalosResult;
use talos_types::talos::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, TalosSessionEnvironment,
};
use talos_types::talos::{TalosContentItem, TokenUsage, TurnExecutionMetrics};
use talos_types::{ModelInfo, ModelSelection, ToolArguments};
use tokio::sync::mpsc;

/// Kernel-private item produced while executing one reasoning loop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum KernelRunItem {
    UserInput {
        text: String,
        #[serde(default, rename = "fileIds")]
        file_ids: Vec<String>,
    },
    Reasoning {
        text: String,
    },
    ExecutionSummary {
        text: String,
    },
    OutputText {
        text: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: ToolArguments,
    },
    ToolResult {
        call_id: String,
        tool_name: String,
        content: Vec<TalosContentItem>,
        #[serde(default)]
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    #[default]
    Execution,
    Planning,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionMode {
    Sequential,
    #[default]
    Parallel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptWarningEventData {
    pub warning: KernelWarning,
    pub agent_id: String,
    pub session_id: String,
    pub phase: TurnPhase,
}

/// One typed event emitted by Agent Kernel during an invocation.
///
/// The payload is the only discriminator; a parallel kind field would permit
/// contradictory values and force every adapter to validate impossible pairs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKernelEvent {
    pub payload: AgentKernelEventPayload,
    pub occurred_at: i64,
}

impl AgentKernelEvent {
    pub fn new(payload: AgentKernelEventPayload) -> Self {
        Self {
            payload,
            occurred_at: Utc::now().timestamp_millis(),
        }
    }
}

/// Closed Kernel event union consumed only by concrete Provider adapters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentKernelEventPayload {
    UserInput(UserInput),
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
    pub usage: TokenUsage,
    pub context: Option<talos_types::ContextUsage>,
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
    pub call_id: String,
    pub tool_name: String,
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

#[derive(Clone)]
pub struct InvocationContext {
    pub session_id: String,
    pub user_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub project_root: Option<PathBuf>,
    pub input: UserInput,
    pub resolved_attachments: Vec<ResolvedAttachment>,
    pub model_selection: ModelSelection,
    pub selected_model_info: ModelInfo,
    pub phase: TurnPhase,
    pub planning_instruction: Option<String>,
    pub session: InvocationSession,
    pub session_environment: Option<TalosSessionEnvironment>,
    pub transcript_service: std::sync::Arc<dyn KernelTranscriptService>,
    pub session_artifact_store: std::sync::Arc<dyn KernelSessionArtifactStore>,
    pub session_prompt_context_source: std::sync::Arc<dyn crate::SessionPromptContextSource>,
    pub memory_progress_store: std::sync::Arc<dyn KernelMemoryProgressStore>,
    pub stream_tx: Option<mpsc::Sender<AgentKernelEvent>>,
}

impl InvocationContext {
    pub fn selected_model_id(&self) -> &str {
        self.selected_model_info.model.id.as_str()
    }

    pub fn session_artifact_store(&self) -> std::sync::Arc<dyn KernelSessionArtifactStore> {
        self.session_artifact_store.clone()
    }

    pub fn session_prompt_context_source(
        &self,
    ) -> std::sync::Arc<dyn crate::SessionPromptContextSource> {
        self.session_prompt_context_source.clone()
    }

    pub fn memory_progress_store(&self) -> std::sync::Arc<dyn KernelMemoryProgressStore> {
        self.memory_progress_store.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelExecutionSettings {
    pub planner_kind: talos_config::PlannerMode,
    pub tool_execution_mode: ToolExecutionMode,
}

impl Default for KernelExecutionSettings {
    fn default() -> Self {
        Self {
            planner_kind: talos_config::PlannerMode::BuiltIn,
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
