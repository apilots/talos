use super::*;

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
    pub invocation_id: String,
    pub agent_id: String,
    pub selected_model: String,
    pub prompt_prefix_messages: Vec<ChatMessage>,
    pub messages: Vec<ChatMessage>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
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
    pub saw_output_delta: bool,
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
            Some("length") => Self::Length,
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
    pub saw_output_delta: bool,
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
            saw_output_delta: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeneralTurnStepRequest {
    pub round: u32,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub parallel_tool_calls: bool,
    pub native_thinking: Option<NativeThinkingConfig>,
    pub stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
}

impl GeneralTurnStepRequest {
    pub fn into_chat_request(self) -> ChatRequest {
        let mut builder = ChatRequest::builder()
            .messages(self.messages)
            .model(self.model);

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
    pub output_items: Vec<TalosOutputItem>,
    pub usage: TalosUsage,
    pub saw_output_delta: bool,
    pub disposition: GeneralTurnStepDisposition,
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
    pub(crate) is_error: bool,
    pub(crate) truncated: bool,
    pub(crate) size_bytes: usize,
}
