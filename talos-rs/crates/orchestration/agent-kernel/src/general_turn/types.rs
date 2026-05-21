use super::*;
use crate::{ToolExecutionMode, ToolExecutionOutput};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

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
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
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
    pub prompt_prefix_messages: Vec<ChatMessage>,
    pub messages: Vec<ChatMessage>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
    pub prompt_warning_state: Option<PromptWarningState>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

#[derive(Debug, Clone)]
pub struct ExecutedTurnArtifacts {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: StopReason,
    pub usage: TalosUsage,
    pub reasoning_log: Vec<String>,
    pub tool_call_log: Vec<ToolCallRecord>,
    pub tool_result_log: Vec<ToolResultLog>,
    pub final_answer_streamed: bool,
    pub execution_summary_streamed: bool,
}

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
    pub messages: Vec<ChatMessage>,
    pub round: u32,
    pub output_items: Vec<TalosOutputItem>,
    pub usage: TalosUsage,
    pub reasoning_log: Vec<String>,
    pub tool_call_log: Vec<ToolCallRecord>,
    pub tool_result_log: Vec<ToolResultLog>,
    pub retrieved_memories: Vec<RetrievedMemoryItem>,
    pub tool_call_count: usize,
    pub tool_error_count: usize,
    pub final_answer_streamed: bool,
    pub execution_summary_streamed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForcedFinalizationCause {
    LoopTerminal,
    ToolRoundBudget,
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

/// Per-turn loop control state for PlanReAct mode.
/// Lives only within a single `general_turn` execution; not persisted to session.
#[derive(Debug, Clone, Default)]
pub struct PlanReActLoopState {
    pub replan_count: u32,
    pub consecutive_no_progress_rounds: u32,
    pub consecutive_read_only_rounds: u32,
}

impl GeneralTurnLoopState {
    pub fn new(_prepared: &PreparedGeneralTurn) -> Self {
        Self {
            messages: Vec::new(),
            round: 0,
            output_items: Vec::new(),
            usage: TalosUsage::default(),
            reasoning_log: Vec::new(),
            tool_call_log: Vec::new(),
            tool_result_log: Vec::new(),
            retrieved_memories: Vec::new(),
            tool_call_count: 0,
            tool_error_count: 0,
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
    pub native_thinking: Option<NativeThinkingConfig>,
    pub pre_final_execution_summary: Option<String>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
    pub streaming_tool_execution: Option<StreamingToolExecutionConfig>,
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

        if let Some(native_thinking) = self.native_thinking {
            builder = builder.native_thinking(native_thinking);
        }

        builder.build()
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnStepResult {
    pub text: String,
    pub reasoning: Vec<String>,
    pub tool_calls: Vec<FunctionCall>,
    pub streamed_tool_outputs: Vec<ToolExecutionOutput>,
    pub model_finish_reason: ModelFinishReason,
    pub output_items: Vec<TalosOutputItem>,
    pub usage: TalosUsage,
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
    pub output_items: Vec<TalosOutputItem>,
    pub tool_messages: Vec<ChatMessage>,
    pub pending_confirmations: Vec<PendingToolConfirmation>,
    pub pending_dynamic_requests: Vec<PendingDynamicToolRequest>,
    pub retrieved_memories: Vec<RetrievedMemoryItem>,
    pub logs: Vec<ToolResultLog>,
    pub error_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingToolConfirmation {
    pub tool_name: String,
    pub call_id: String,
    pub prompt: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingDynamicToolRequest {
    pub tool_name: String,
    pub call_id: String,
    pub arguments: Value,
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
