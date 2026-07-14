use super::*;
use crate::{ToolExecutionMode, ToolExecutionOutput};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use talos_skills::{SkillRoots, SkillSelector};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionCompactionStatus {
    Normal,
    Compressing,
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

    pub fn compressing() -> Self {
        Self {
            status: SessionCompactionStatus::Compressing,
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
    pub metrics: TurnExecutionMetrics,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
    pub outcome: GeneralTurnOutcome,
}

#[derive(Debug, Clone)]
pub struct PreparedGeneralTurn {
    pub session_id: String,
    pub message_id: String,
    pub invocation_id: String,
    pub agent_id: String,
    pub project_root: Option<PathBuf>,
    pub selected_model: String,
    pub max_output_tokens: Option<u32>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub speed: ModelSpeed,
    pub prompt_prefix_messages: Vec<ChatMessage>,
    pub messages: Vec<ChatMessage>,
    pub maintenance_usage: TokenUsage,
    pub tool_definitions: Vec<ToolDefinition>,
    pub tool_selection_request: talos_tools::ToolSelectionRequest,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
    pub prompt_warning_state: Option<PromptWarningState>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

#[derive(Debug, Clone)]
pub struct ExecutedTurnArtifacts {
    pub output_text: String,
    pub output_items: Vec<RunItem>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
    pub metrics: TurnExecutionMetrics,
    pub reasoning_log: Vec<String>,
    pub tool_call_log: Vec<ToolCallRecord>,
    pub tool_result_log: Vec<ToolResultLog>,
    pub pending_dynamic_requests: Vec<PendingDynamicToolRequest>,
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
    DynamicToolResult(PendingDynamicToolRequest),
}

#[derive(Debug, Clone)]
pub enum GeneralTurnResume {
    DynamicToolResult(DynamicToolResultResume),
}

#[derive(Debug, Clone)]
pub struct DynamicToolResultResume {
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
    pub original_call: Option<ResumeToolCall>,
    pub replay_dynamic_tool_results:
        std::collections::HashMap<String, crate::execution::ReplayDynamicToolResult>,
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

    fn before_dynamic_resume(&self, _resume: &mut DynamicToolResultResume) -> TalosResult<()> {
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
    pub reasoning_log: Vec<String>,
    pub tool_call_log: Vec<ToolCallRecord>,
    pub tool_result_log: Vec<ToolResultLog>,
    pub retrieved_memories: Vec<RetrievedMemoryItem>,
    pub tool_call_count: usize,
    pub tool_error_count: usize,
    pub pending_dynamic_requests: Vec<PendingDynamicToolRequest>,
    pub final_answer_streamed: bool,
    pub execution_summary_streamed: bool,
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
            reasoning_log: Vec::new(),
            tool_call_log: Vec::new(),
            tool_result_log: Vec::new(),
            retrieved_memories: Vec::new(),
            tool_call_count: 0,
            tool_error_count: 0,
            pending_dynamic_requests: Vec::new(),
            final_answer_streamed: false,
            execution_summary_streamed: false,
        }
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
    pub execution_context: ToolExecutionContext,
    pub tool_registry: Arc<dyn ToolExecutorPort>,
}

impl fmt::Debug for StreamingToolExecutionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingToolExecutionConfig")
            .field("execution_mode", &self.execution_mode)
            .field("execution_context", &self.execution_context)
            .field("tool_registry", &"<dyn ToolExecutorPort>")
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnToolBatch {
    pub output_items: Vec<RunItem>,
    pub tool_messages: Vec<ChatMessage>,
    pub pending_dynamic_requests: Vec<PendingDynamicToolRequest>,
    pub retrieved_memories: Vec<RetrievedMemoryItem>,
    pub logs: Vec<ToolResultLog>,
    pub error_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingDynamicToolRequest {
    pub tool_name: String,
    pub call_id: String,
    pub arguments: Value,
    pub reasoning_continuation: Option<ReasoningContinuation>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ToolResultLog {
    pub(crate) call_id: String,
    pub(crate) tool: String,
    pub(crate) payload: Value,
    pub(crate) envelope: Value,
    pub(crate) is_error: bool,
    pub(crate) truncated: bool,
    pub(crate) size_bytes: usize,
    pub(crate) semantic_events: Vec<String>,
    pub(crate) next_turn_failure: Option<Value>,
}
