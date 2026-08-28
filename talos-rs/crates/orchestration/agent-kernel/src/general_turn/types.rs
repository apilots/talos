use super::*;
use crate::{ToolExecutionMode, ToolExecutionOutput};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use talos_skills::{SkillRoots, SkillSelector};
use talos_types::talos::ToolArguments;
use talos_types::{UserActionParams, UserActionResponse};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: ToolArguments,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionMemoryOutput {
    pub title: String,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub preferences: Vec<String>,
    #[serde(default)]
    pub open_tasks: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub memory_candidates: Vec<talos_memory::MemoryCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(super) enum DreamMemoryModelOutput {
    Compaction {
        title: String,
        #[serde(default)]
        facts: Vec<String>,
        #[serde(default)]
        preferences: Vec<String>,
        #[serde(default)]
        open_tasks: Vec<String>,
        #[serde(default)]
        risks: Vec<String>,
        #[serde(default)]
        memory_candidates: Vec<talos_memory::MemoryCandidate>,
    },
    Maintenance {
        #[serde(default)]
        memory_candidates: Vec<talos_memory::MemoryCandidate>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionCompactionStatus {
    Normal,
    Recovery,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionCompactionBlockedReason {
    PromptBudgetExhaustedAfterCompaction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionCompactionState {
    pub status: SessionCompactionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<SessionCompactionBlockedReason>,
}

impl SessionCompactionState {
    pub fn normal() -> Self {
        Self {
            status: SessionCompactionStatus::Normal,
            blocked_reason: None,
        }
    }

    pub fn recovery() -> Self {
        Self {
            status: SessionCompactionStatus::Recovery,
            blocked_reason: None,
        }
    }

    pub fn blocked_prompt_budget_exhausted() -> Self {
        Self {
            status: SessionCompactionStatus::Blocked,
            blocked_reason: Some(
                SessionCompactionBlockedReason::PromptBudgetExhaustedAfterCompaction,
            ),
        }
    }

    pub fn is_blocked(&self) -> bool {
        self.status == SessionCompactionStatus::Blocked
    }

    pub fn is_recovery(&self) -> bool {
        self.status == SessionCompactionStatus::Recovery
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnResult {
    pub output_text: String,
    pub output_items: Vec<RunItem>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    pub last_model_usage: TokenUsage,
    pub metrics: TurnExecutionMetrics,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
    pub outcome: GeneralTurnOutcome,
    pub artifact_seq: u64,
}

#[derive(Debug, Clone)]
pub struct PreparedGeneralTurn {
    pub session_id: String,
    pub user_id: String,
    pub message_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub project_root: Option<PathBuf>,
    pub selected_model: String,
    pub max_output_tokens: Option<u32>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub speed: ModelSpeed,
    pub attachment_tokens: usize,
    pub prompt_prefix_messages: Vec<ChatMessage>,
    pub messages: Vec<ChatMessage>,
    pub maintenance_usage: TokenUsage,
    pub tool_definitions: Vec<ToolDefinition>,
    pub tool_selection_request: talos_tools::ToolSelectionRequest,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
    pub prompt_warning_state: Option<PromptWarningState>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
    pub message_rx: Option<Arc<std::sync::Mutex<mpsc::Receiver<crate::ActiveTurnInputDelivery>>>>,
    /// Harness-owned Run capabilities propagated to Tool execution.
    pub run_tasks: talos_run_control::RunTaskGroup,
}

#[derive(Debug, Clone)]
pub struct ExecutedTurnArtifacts {
    pub output_text: String,
    pub output_items: Vec<RunItem>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    pub last_model_usage: TokenUsage,
    pub metrics: TurnExecutionMetrics,
    pub reasoning_log: Vec<String>,
    pub tool_call_log: Vec<ToolCallRecord>,
    pub tool_result_log: Vec<ToolResultLog>,
    pub pending_user_action: Option<PendingUserActionRequest>,
    pub final_answer_streamed: bool,
    pub execution_summary_streamed: bool,
}

#[derive(Debug, Clone)]
pub enum GeneralTurnOutcome {
    Completed {
        output_text: String,
        output_items: Vec<RunItem>,
        stop_reason: StopReason,
        usage: TokenUsage,
        metrics: TurnExecutionMetrics,
    },
    Waiting {
        output_items: Vec<RunItem>,
        pending_action: PendingGeneralTurnAction,
        stop_reason: StopReason,
        usage: TokenUsage,
        metrics: TurnExecutionMetrics,
    },
}

#[derive(Debug, Clone)]
pub enum PendingGeneralTurnAction {
    UserAction(PendingUserActionRequest),
}

#[derive(Debug, Clone)]
pub enum GeneralTurnResume {
    UserAction(UserActionResume),
}

#[derive(Debug, Clone)]
pub struct UserActionResume {
    pub call_id: String,
    pub tool_name: String,
    pub response: UserActionResponse,
    pub sandbox_request_fingerprint: Option<String>,
    pub original_call: Option<ResumeToolCall>,
    pub tool_calls: Vec<FunctionCall>,
    pub tool_messages: Vec<ChatMessage>,
    pub deferred_tool_calls: Vec<FunctionCall>,
    pub replay_dynamic_tool_results:
        std::collections::HashMap<String, crate::execution::ReplayDynamicToolResult>,
    pub execution_metrics: TurnExecutionMetrics,
}

#[derive(Debug, Clone)]
pub struct ResumeToolCall {
    pub item_id: Option<String>,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: DynamicToolArguments,
    pub reasoning_continuation: Option<ReasoningContinuation>,
}

#[derive(Debug, Clone)]
pub struct GeneralTurnPolicyStop {
    pub output_text: String,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneralTurnPolicyFinalAcceptance {
    Accept,
    Reject,
}

pub trait GeneralTurnPolicy: Send + Sync {
    fn before_step_request(&self, _request: &mut GeneralTurnStepRequest) -> TalosResult<()> {
        Ok(())
    }

    fn after_step_result(
        &self,
        _result: &mut GeneralTurnStepResult,
    ) -> TalosResult<Option<GeneralTurnPolicyStop>> {
        Ok(None)
    }

    fn accepts_final_response(&self, _text: &str) -> GeneralTurnPolicyFinalAcceptance {
        GeneralTurnPolicyFinalAcceptance::Accept
    }

    fn before_user_action_resume(&self, _resume: &mut UserActionResume) -> TalosResult<()> {
        Ok(())
    }

    fn after_tool_batch(
        &self,
        _batch: &mut GeneralTurnToolBatch,
    ) -> TalosResult<Option<GeneralTurnPolicyStop>> {
        Ok(None)
    }
}

#[derive(Debug, Default)]
pub struct BuiltInGeneralTurnPolicy;

impl GeneralTurnPolicy for BuiltInGeneralTurnPolicy {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFinishReason {
    Stop,
    ToolCalls,
    Length,
    Other,
}

impl ModelFinishReason {
    pub(crate) fn from_provider(value: Option<&str>, has_tool_calls: bool) -> Self {
        if has_tool_calls {
            return Self::ToolCalls;
        }
        match value {
            Some("stop") | None => Self::Stop,
            Some("tool_calls") => Self::ToolCalls,
            Some("length") | Some("max_tokens") => Self::Length,
            Some(_) => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneralTurnStepDisposition {
    ContinueWithTools,
    FinalResponse,
}

#[derive(Debug, Clone)]
pub struct GeneralTurnLoopState {
    pub started_at: Instant,
    pub messages: Vec<ChatMessage>,
    pub round: u32,
    pub output_items: Vec<RunItem>,
    pub usage: TokenUsage,
    pub last_model_usage: TokenUsage,
    pub reasoning_log: Vec<String>,
    pub tool_call_log: Vec<ToolCallRecord>,
    pub tool_result_log: Vec<ToolResultLog>,
    pub(crate) retrieved_memories: Vec<talos_types::MemoryInfo>,
    pub tool_call_count: usize,
    pub tool_result_count: usize,
    pub tool_error_count: usize,
    pub pending_user_action: Option<PendingUserActionRequest>,
    pub final_answer_streamed: bool,
    pub execution_summary_streamed: bool,
    pub elapsed_duration_ms: u64,
    pub active_turn_attachment_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForcedFinalizationCause {
    RepeatedReadOnlyInvestigation,
    EmptyFinalResponseAfterTools,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallFingerprint {
    pub tool_name: String,
    pub canonical_arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnlyRoundFingerprint {
    pub calls: Vec<ToolCallFingerprint>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolLoopGuardState {
    pub last_read_only_round: Option<ReadOnlyRoundFingerprint>,
    pub consecutive_equivalent_read_only_rounds: u32,
}

impl GeneralTurnLoopState {
    pub fn new(prepared: &PreparedGeneralTurn) -> Self {
        Self {
            started_at: Instant::now(),
            messages: Vec::new(),
            round: 0,
            output_items: Vec::new(),
            usage: prepared.maintenance_usage.clone(),
            last_model_usage: TokenUsage::default(),
            reasoning_log: Vec::new(),
            tool_call_log: Vec::new(),
            tool_result_log: Vec::new(),
            retrieved_memories: Vec::new(),
            tool_call_count: 0,
            tool_result_count: 0,
            tool_error_count: 0,
            pending_user_action: None,
            final_answer_streamed: false,
            execution_summary_streamed: false,
            elapsed_duration_ms: 0,
            active_turn_attachment_tokens: 0,
        }
    }

    pub fn restore_execution_metrics(&mut self, metrics: &TurnExecutionMetrics) {
        self.round = metrics.model_rounds.saturating_sub(1);
        self.tool_call_count = metrics.tool_calls as usize;
        self.tool_result_count =
            metrics.tool_successes.saturating_add(metrics.tool_errors) as usize;
        self.tool_error_count = metrics.tool_errors as usize;
        self.usage = metrics.usage.clone();
        self.elapsed_duration_ms = metrics.duration_ms.unwrap_or_default();
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnStepRequest {
    pub round: u32,
    pub call_mode: KernelModelCallMode,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub messages: Vec<ChatMessage>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub parallel_tool_calls: bool,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub speed: ModelSpeed,
    pub pre_final_execution_summary: Option<String>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
    pub streaming_tool_execution: Option<StreamingToolExecutionConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct GeneralTurnSkillOptions {
    pub selected: Vec<SkillSelector>,
    pub roots: Option<SkillRoots>,
}

impl GeneralTurnStepRequest {
    pub fn into_chat_request(self) -> ChatRequest {
        let mut builder = ChatRequest::builder()
            .messages(self.messages)
            .model(self.model);

        if let Some(max_tokens) = self.max_tokens {
            builder = builder.max_tokens(max_tokens);
        }

        if !self.tool_definitions.is_empty() {
            builder = builder
                .tools(self.tool_definitions)
                .tool_choice(ToolChoice::Auto)
                .parallel_tool_calls(self.parallel_tool_calls);
        }

        builder = builder.reasoning_effort(self.reasoning_effort);

        builder = builder.speed(self.speed);

        builder.build()
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnStepResult {
    pub text: String,
    pub reasoning: ReasoningOutput,
    pub tool_calls: Vec<FunctionCall>,
    pub streamed_tool_outputs: Vec<ToolExecutionOutput>,
    pub model_finish_reason: ModelFinishReason,
    pub output_items: Vec<RunItem>,
    pub usage: TokenUsage,
    pub final_answer_streamed: bool,
    pub execution_summary_streamed: bool,
    pub disposition: GeneralTurnStepDisposition,
}

#[derive(Clone)]
pub struct StreamingToolExecutionConfig {
    pub execution_mode: ToolExecutionMode,
    pub max_parallelism: usize,
    pub execution_context: ToolExecutionContext,
    pub tool_registry: Arc<dyn ToolExecutorPort>,
}

impl fmt::Debug for StreamingToolExecutionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingToolExecutionConfig")
            .field("execution_mode", &self.execution_mode)
            .field("max_parallelism", &self.max_parallelism)
            .field("execution_context", &self.execution_context)
            .field("tool_registry", &"<dyn ToolExecutorPort>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnToolBatch {
    pub output_items: Vec<RunItem>,
    pub tool_messages: Vec<ChatMessage>,
    pub pending_user_action: Option<PendingUserActionRequest>,
    pub logs: Vec<ToolResultLog>,
    pub error_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingUserActionRequest {
    pub action_id: String,
    pub tool_name: String,
    pub call_id: String,
    pub arguments: Value,
    pub params: UserActionParams,
    pub sandbox_request_fingerprint: Option<String>,
    pub reasoning_continuation: Option<ReasoningContinuation>,
    pub tool_calls: Vec<FunctionCall>,
    pub tool_messages: Vec<ChatMessage>,
    pub deferred_tool_calls: Vec<FunctionCall>,
}

#[derive(Debug, Clone)]
pub struct ToolResultLog {
    pub(crate) call_id: String,
    pub(crate) tool: String,
    pub(crate) payload: Value,
    pub(crate) is_error: bool,
}
