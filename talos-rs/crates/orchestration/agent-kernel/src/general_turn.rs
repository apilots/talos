use crate::tool_execution::ToolExecutionContext;
use crate::{
    warnings::derive_prompt_warnings, ExecutionSummaryStreamData, InvocationContext, KernelEvent,
    KernelEventData, KernelEventLabel, KernelEventType, KernelOutputDeltaSource, KernelStreamEvent,
    KernelStreamEventData, KernelStreamEventType, KernelThinkingSource, KernelThinkingStage,
    KernelWarning, OutputDeltaStreamData, PromptWarningEventData, StopReason,
    ThinkingDeltaStreamData, ToolCallStartStreamData, ToolExecutionInput, ToolExecutor,
    ToolResultStreamData,
};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatMessage, ChatRequest, FunctionCall, LlmProvider, NativeThinkingConfig, ToolChoice,
    ToolDefinition, ToolStreamEvent,
};
use talos_memory::{
    Memory, MemoryGetResult, MemorySearchResponse, MemoryStore, RecordHeader, RetrievedMemoryItem,
    SessionPromptContextConfig, SummaryRecord, TalosMessage, TalosMessageRole, TalosTurn,
    ToolCallRecord, ToolResultRecord,
};
use talos_prompt::{
    render_prompt_assembly, PromptBudget, PromptBuildConfig, PromptBuilder, PromptDiagnostics,
    PromptLoadPolicy, PromptPhase,
};
use talos_tools::{
    ToolDefinitionProvider, ToolDispatchOutcome, ToolExecutorPort, ToolOutputPayload, ToolPending,
};
use talos_types::talos::{TalosContentItem, TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;
use tracing::warn;
use uuid::Uuid;

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
    fn from_provider(value: Option<&str>, has_tool_calls: bool) -> Self {
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

pub async fn flush_session_memory(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
    enforce_threshold: bool,
    persist_session_summary: bool,
    flush_workspace_memory: bool,
) -> TalosResult<()> {
    if persist_session_summary || flush_workspace_memory {
        if enforce_threshold {
            let threshold = kernel_config.memory.compaction_threshold_bytes as u64;
            if threshold == 0 {
                return Ok(());
            }
            let session_bytes = memory_store.session_size_bytes(session_id).await?;
            if session_bytes < threshold {
                return Ok(());
            }
        }
    } else {
        return Ok(());
    }

    let context = memory_store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                recent_messages_limit: 48,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 0,
            },
        )
        .await?;
    if context.recent_messages.is_empty() {
        return Ok(());
    }
    let transcript = context
        .recent_messages
        .iter()
        .map(|message| match message.role {
            TalosMessageRole::User => format!("User: {}", message.text.trim()),
            TalosMessageRole::Assistant => format!("Assistant: {}", message.text.trim()),
            TalosMessageRole::System => format!("System: {}", message.text.trim()),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.trim().is_empty() {
        return Ok(());
    }

    let selected_model = selected_model.to_string();
    let llm_provider_for_summary = llm_provider.clone();
    let request = ChatRequest::builder()
        .model(selected_model)
        .messages(vec![
            ChatMessage::system(
                "Summarize the conversation into durable facts, user preferences, open tasks, and \
                 unresolved risks. Keep under 220 words.",
            ),
            ChatMessage::user(format!("Conversation transcript:\n{}", transcript)),
        ])
        .build();
    let response = llm_provider_for_summary.chat(request).await?;
    let summary = response.content.trim().to_string();
    if summary.is_empty() {
        return Ok(());
    }
    let latest_seq = memory_store.latest_seq(session_id).await?;
    let source_turn_count = context
        .recent_messages
        .iter()
        .filter_map(|item| item.header.turn_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    if persist_session_summary {
        memory_store
            .save_summary(&SummaryRecord {
                version: 2,
                header: RecordHeader {
                    id: Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    turn_id: None,
                    timestamp: Utc::now(),
                },
                covers_until_seq: latest_seq,
                source_turn_count,
                content: summary.clone(),
            })
            .await?;
    }

    if flush_workspace_memory {
        memory_store
            .append_workspace_memory(&talos_memory::WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                title: Some("Durable Memory".to_string()),
                content: summary.clone(),
            })
            .await?;
    }

    Ok(())
}

async fn compact_workspace_context_if_needed(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<()> {
    flush_session_memory(
        session_id,
        selected_model,
        llm_provider,
        memory_store,
        kernel_config,
        true,
        true,
        kernel_config.memory.workspace_memory_flush_enabled
            && kernel_config.memory.workspace_memory_flush_on_compaction,
    )
    .await
}

fn build_execution_summary(tool_call_count: usize, tool_error_count: usize) -> String {
    if tool_call_count == 0 {
        "Kernel summary: answered directly without invoking tools.".to_string()
    } else if tool_error_count == 0 {
        format!(
            "Kernel summary: executed {} tool call(s) successfully before final response.",
            tool_call_count
        )
    } else {
        format!(
            "Kernel summary: executed {} tool call(s), {} call(s) returned errors; final response generated with available results.",
            tool_call_count, tool_error_count
        )
    }
}

#[derive(Debug, Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments_buffer: String,
    final_arguments: Option<Value>,
}

#[derive(Debug, Default)]
struct StreamRound {
    content: String,
    reasoning: String,
    tool_calls: Vec<FunctionCall>,
    finish_reason: Option<String>,
    saw_output_delta: bool,
    usage: TalosUsage,
}

#[derive(Debug, Clone)]
pub struct ToolResultLog {
    call_id: String,
    tool: String,
    payload: Value,
    is_error: bool,
    truncated: bool,
    size_bytes: usize,
}

fn parse_retrieved_memory_items(tool_name: &str, payload: &Value) -> Vec<RetrievedMemoryItem> {
    match tool_name {
        "memory_search" => match serde_json::from_value::<MemorySearchResponse>(payload.clone()) {
            Ok(response) => response.into_retrieved_items(),
            Err(err) => {
                warn!(
                    tool_name = %tool_name,
                    error = %err,
                    payload = %payload,
                    "failed to parse memory_search payload"
                );
                Vec::new()
            }
        },
        "memory_get" => match serde_json::from_value::<MemoryGetResult>(payload.clone()) {
            Ok(result) if !result.text.trim().is_empty() => vec![RetrievedMemoryItem {
                path: result.path,
                snippet: result.text.trim().to_string(),
                citation: None,
                score: 1.0,
            }],
            Ok(_) => Vec::new(),
            Err(err) => {
                warn!(
                    tool_name = %tool_name,
                    error = %err,
                    payload = %payload,
                    "failed to parse memory_get payload"
                );
                Vec::new()
            }
        },
        _ => Vec::new(),
    }
}

fn build_retrieved_memories_message(
    items: &[RetrievedMemoryItem],
    max_items: usize,
    max_chars: usize,
) -> Option<ChatMessage> {
    if items.is_empty() || max_items == 0 || max_chars == 0 {
        return None;
    }
    let mut content = String::new();
    for item in items.iter().take(max_items) {
        let line = format!(
            "- {}{}: {}",
            item.path,
            item.citation
                .as_ref()
                .map(|citation| format!(" ({citation})"))
                .unwrap_or_default(),
            item.snippet.trim()
        );
        if content.len() + line.len() + 1 > max_chars {
            break;
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&line);
    }
    if content.is_empty() {
        None
    } else {
        Some(ChatMessage::system(format!(
            "Retrieved memories:\n{}",
            content
        )))
    }
}

fn split_prompt_prefix_messages(messages: &[ChatMessage], input: &str) -> Vec<ChatMessage> {
    let Some(last_message) = messages.last() else {
        return Vec::new();
    };
    if last_message.role == talos_llm_provider::Role::User && last_message.content == input {
        return messages[..messages.len().saturating_sub(1)].to_vec();
    }
    messages.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ContextStateBags, InvocationExecutionState, InvocationLimits, KernelEventData,
        KernelStreamEventData, KernelStreamEventType, TurnPhase,
    };
    use async_trait::async_trait;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use talos_llm_provider::{
        ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, ToolStream,
        ToolStreamEvent, Usage,
    };
    use talos_tools::{register_core_local_tools, ToolRegistry};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    struct NoopLlmProvider;

    #[derive(Default)]
    struct ExecLoopProvider {
        round: AtomicUsize,
    }

    #[derive(Default)]
    struct ApprovalLoopProvider {
        round: AtomicUsize,
    }

    struct StreamingToolCallIdProvider;

    #[async_trait]
    impl LlmProvider for NoopLlmProvider {
        fn name(&self) -> &str {
            "noop"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: "ok".to_string(),
                model: "noop".to_string(),
                usage: Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "noop"
        }
    }

    #[async_trait]
    impl LlmProvider for ExecLoopProvider {
        fn name(&self) -> &str {
            "exec-loop"
        }

        async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            self.chat_with_tools(request).await
        }

        async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            let round = self.round.fetch_add(1, Ordering::SeqCst);
            match round {
                0 => Ok(ChatResponse {
                    content: "Open a shell session first.".to_string(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_exec",
                        "exec_command",
                        json!({
                            "cmd": "printf 'ready\\n'; read line; printf 'done:%s\\n' \"$line\"",
                            "shell": "/bin/sh",
                            "login": false,
                            "yield_time_ms": 50
                        }),
                    )]),
                }),
                1 => {
                    let tool_message = request
                        .messages
                        .iter()
                        .find(|msg| {
                            msg.role == talos_llm_provider::Role::Tool
                                && msg.tool_call_id.as_deref() == Some("call_exec")
                        })
                        .expect("exec_command tool message should exist");
                    let payload: Value = serde_json::from_str(&tool_message.content)
                        .expect("exec_command payload should parse");
                    let session_id = payload
                        .get("session_id")
                        .and_then(Value::as_i64)
                        .expect("session_id should exist");
                    assert_eq!(
                        payload.get("status").and_then(Value::as_str),
                        Some("running")
                    );
                    assert!(payload
                        .get("output")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .contains("ready"));
                    Ok(ChatResponse {
                        content: "Write stdin to finish.".to_string(),
                        model: request.model,
                        usage: Usage::default(),
                        finish_reason: Some("tool_calls".to_string()),
                        tool_calls: Some(vec![FunctionCall::new(
                            "call_write",
                            "write_stdin",
                            json!({
                                "session_id": session_id,
                                "chars": "ok\n",
                                "yield_time_ms": 50
                            }),
                        )]),
                    })
                }
                _ => {
                    let tool_message = request
                        .messages
                        .iter()
                        .find(|msg| {
                            msg.role == talos_llm_provider::Role::Tool
                                && msg.tool_call_id.as_deref() == Some("call_write")
                        })
                        .expect("write_stdin tool message should exist");
                    let payload: Value = serde_json::from_str(&tool_message.content)
                        .expect("write_stdin payload should parse");
                    let output = payload
                        .get("output")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Ok(ChatResponse {
                        content: format!("Final answer after stdin write: {}", output.trim()),
                        model: request.model,
                        usage: Usage::default(),
                        finish_reason: Some("stop".to_string()),
                        tool_calls: None,
                    })
                }
            }
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "exec-loop"
        }
    }

    #[async_trait]
    impl LlmProvider for ApprovalLoopProvider {
        fn name(&self) -> &str {
            "approval-loop"
        }

        async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            self.chat_with_tools(request).await
        }

        async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
            let round = self.round.fetch_add(1, Ordering::SeqCst);
            match round {
                0 => Ok(ChatResponse {
                    content: "Request escalated shell access.".to_string(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_exec",
                        "exec_command",
                        json!({
                            "cmd": "echo hi",
                            "shell": "/bin/sh",
                            "login": false,
                            "sandbox_permissions": "require_escalated",
                            "justification": "Need unrestricted shell access."
                        }),
                    )]),
                }),
                _ => panic!("approval flow should stop before a second model round"),
            }
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "approval-loop"
        }
    }

    #[async_trait]
    impl LlmProvider for StreamingToolCallIdProvider {
        fn name(&self) -> &str {
            "streaming-tool-call-id"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            panic!("chat should not be called in stream_round test");
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
            let (tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(8);
            tokio::spawn(async move {
                tx.send(Ok(ToolStreamEvent::ToolCallStart {
                    index: 0,
                    id: Some(String::new()),
                    name: Some("memory_search".to_string()),
                }))
                .await
                .expect("tool start should send");
                tx.send(Ok(ToolStreamEvent::ToolCallEnd {
                    index: 0,
                    id: Some(String::new()),
                    name: Some("memory_search".to_string()),
                    arguments: json!({ "query": "talos项目" }),
                }))
                .await
                .expect("tool end should send");
                tx.send(Ok(ToolStreamEvent::Done {
                    finish_reason: Some("tool_calls".to_string()),
                    usage: None,
                }))
                .await
                .expect("done event should send");
            });
            Ok(ToolStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "streaming-tool-call-id"
        }
    }

    fn build_ctx_with_stream(
        stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
    ) -> InvocationContext {
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
            stream_tx,
        }
    }

    async fn seed_recent_user_message(memory_store: &MemoryStore) {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: "turn-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                messages: vec![TalosMessage {
                    header: RecordHeader {
                        id: "turn-1:u".to_string(),
                        session_id: "s1".to_string(),
                        turn_id: Some("turn-1".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::User,
                    text: "history that should be dropped".to_string(),
                }],
                reasoning: Vec::new(),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
            })
            .await
            .expect("append should succeed");
    }

    #[test]
    fn test_parse_retrieved_memory_items_from_memory_search() {
        let payload = json!(MemorySearchResponse {
            results: vec![talos_memory::MemorySearchResult {
                path: "memory/2026-03-24.md".to_string(),
                snippet: "launch on friday".to_string(),
                citation: Some("memory/2026-03-24.md#L1-L1".to_string()),
                score: 0.9,
                start_line: 1,
                end_line: 1,
            }],
        });

        let items = parse_retrieved_memory_items("memory_search", &payload);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "memory/2026-03-24.md");
        assert_eq!(items[0].score, 0.9);
    }

    #[test]
    fn test_parse_retrieved_memory_items_ignores_invalid_search_rows() {
        let payload = json!({
            "results": [
                {
                    "path": "memory/2026-03-25.md",
                    "snippet": "missing score"
                }
            ]
        });

        let items = parse_retrieved_memory_items("memory_search", &payload);
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_retrieved_memory_items_from_memory_get() {
        let payload = json!(MemoryGetResult {
            path: "MEMORY.md".to_string(),
            text: "remember tea preference".to_string(),
            from: 1,
            lines: 5,
        });

        let items = parse_retrieved_memory_items("memory_get", &payload);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "MEMORY.md");
    }

    #[tokio::test]
    async fn test_run_general_turn_emits_prompt_warning_event() {
        let provider = Arc::new(NoopLlmProvider);
        let tool_registry = Arc::new(ToolRegistry::new());
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
        seed_recent_user_message(memory_store.as_ref()).await;
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.prompt.max_dynamic_tokens = 1;
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let mut ctx = build_ctx_with_stream(None);

        let result = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "hello",
            provider,
            tool_registry.clone(),
            tool_registry,
            memory_store,
            &config,
        )
        .await
        .expect("general turn should succeed");

        assert!(!result.warnings.is_empty());
        assert!(ctx.events.iter().any(|event| {
            matches!(event.event_type, KernelEventType::PromptWarning)
                && matches!(event.label, KernelEventLabel::PromptWarning)
                && matches!(event.data, KernelEventData::PromptWarning(_))
        }));
    }

    #[tokio::test]
    async fn test_run_general_turn_streams_prompt_warning_event() {
        let provider = Arc::new(NoopLlmProvider);
        let tool_registry = Arc::new(ToolRegistry::new());
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
        seed_recent_user_message(memory_store.as_ref()).await;
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.prompt.max_dynamic_tokens = 1;
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let (tx, mut rx) = mpsc::channel(16);
        let mut ctx = build_ctx_with_stream(Some(tx));

        let _ = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "hello",
            provider,
            tool_registry.clone(),
            tool_registry,
            memory_store,
            &config,
        )
        .await
        .expect("general turn should succeed");

        let mut saw_prompt_warning = false;
        while let Ok(event) = rx.try_recv() {
            if matches!(event.event_type, KernelStreamEventType::PromptWarning)
                && matches!(event.data, KernelStreamEventData::PromptWarning(_))
            {
                saw_prompt_warning = true;
                break;
            }
        }
        assert!(saw_prompt_warning, "stream should include prompt.warning");
    }

    #[tokio::test]
    async fn test_run_general_turn_exec_command_then_write_stdin() {
        let provider = Arc::new(ExecLoopProvider::default());
        let tool_registry = Arc::new(ToolRegistry::new());
        register_core_local_tools(tool_registry.as_ref());

        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
        let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let mut ctx = build_ctx_with_stream(None);

        let result = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "Use exec_command, then continue with write_stdin.",
            provider.clone(),
            tool_registry.clone(),
            tool_registry,
            memory_store,
            &config,
        )
        .await
        .expect("general turn should succeed");

        assert!(result.output_text.contains("done:ok"));
        assert!(result
            .output_items
            .iter()
            .any(|item| matches!(item, talos_types::talos::TalosOutputItem::ToolResult { .. })));
        assert!(provider.round.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn test_run_general_turn_stops_on_pending_tool_confirmation() {
        let tool_registry = Arc::new(ToolRegistry::new());
        register_core_local_tools(tool_registry.as_ref());

        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
        let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let mut ctx = build_ctx_with_stream(None);

        let provider = Arc::new(ApprovalLoopProvider::default());
        let result = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "Use exec_command with escalated permissions.",
            provider.clone(),
            tool_registry.clone(),
            tool_registry,
            memory_store,
            &config,
        )
        .await
        .expect("general turn should succeed");

        assert_eq!(result.output_text, "Tool confirmation pending.");
        assert!(result.output_items.iter().any(|item| matches!(
            item,
            talos_types::talos::TalosOutputItem::ToolResult { is_error: true, .. }
        )));
        assert_eq!(provider.round.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_stream_round_normalizes_empty_tool_call_id_before_emitting_start_event() {
        let provider = Arc::new(StreamingToolCallIdProvider);
        let request = ChatRequest::new_user("Search workspace memory", "streaming-tool-call-id");
        let (tx, mut rx) = mpsc::channel(8);

        let result = stream_round(provider, request, 0, Some(&tx))
            .await
            .expect("stream round should succeed");

        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "tool-0-0");

        let mut started_call_id = None;
        while let Ok(event) = rx.try_recv() {
            if let KernelStreamEventData::ToolCallStart(data) = event.data {
                started_call_id = data.call_id;
                break;
            }
        }

        assert_eq!(started_call_id.as_deref(), Some("tool-0-0"));
    }
}

async fn emit_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    event_type: KernelStreamEventType,
    data: KernelStreamEventData,
) {
    if let Some(tx) = stream_tx {
        let _ = tx.send(KernelStreamEvent::new(event_type, data)).await;
    }
}

async fn emit_prompt_warning_events(ctx: &mut InvocationContext, warnings: &[KernelWarning]) {
    for warning in warnings {
        let payload = PromptWarningEventData {
            warning: warning.clone(),
            agent_id: ctx.agent_id.clone(),
            session_id: ctx.session_id.clone(),
            phase: ctx.phase.clone(),
        };
        ctx.events.push(KernelEvent::new(
            KernelEventType::PromptWarning,
            KernelEventLabel::PromptWarning,
            KernelEventData::PromptWarning(payload.clone()),
        ));
        emit_stream_event(
            ctx.stream_tx.as_ref(),
            KernelStreamEventType::PromptWarning,
            KernelStreamEventData::PromptWarning(payload),
        )
        .await;
    }
}

fn finalize_tool_calls(
    round: u32,
    pending_calls: BTreeMap<usize, PendingToolCall>,
) -> Vec<FunctionCall> {
    let mut calls = Vec::new();

    for (index, call) in pending_calls {
        let Some(name) = call.name.filter(|value| !value.trim().is_empty()) else {
            continue;
        };

        let id = normalized_tool_call_id(round, index, call.id);
        let arguments = if let Some(value) = call.final_arguments {
            value
        } else if call.arguments_buffer.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<Value>(&call.arguments_buffer)
                .unwrap_or_else(|_| Value::String(call.arguments_buffer))
        };

        calls.push(FunctionCall::new(id, name, arguments));
    }

    calls
}

fn normalized_tool_call_id(round: u32, index: usize, id: Option<String>) -> String {
    id.filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("tool-{}-{}", round, index))
}

async fn stream_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
    round: u32,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<StreamRound> {
    let mut stream = llm_provider.stream_with_tools(request).await?;
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut finish_reason = None;
    let mut pending_calls: BTreeMap<usize, PendingToolCall> = BTreeMap::new();
    let mut saw_output_delta = false;
    let mut usage = TalosUsage::default();

    while let Some(event) = stream.next().await {
        match event? {
            ToolStreamEvent::TextDelta { delta } => {
                content.push_str(&delta);
                saw_output_delta = true;
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::OutputDelta,
                    KernelStreamEventData::OutputDelta(OutputDeltaStreamData {
                        delta,
                        round: Some(round),
                        source: Some(KernelOutputDeltaSource::Provider),
                    }),
                )
                .await;
            }
            ToolStreamEvent::ReasoningDelta { delta } => {
                reasoning.push_str(&delta);
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::ThinkingDelta,
                    KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                        delta,
                        source: KernelThinkingSource::Provider,
                        stage: KernelThinkingStage::ModelReasoning,
                        round: Some(round),
                    }),
                )
                .await;
            }
            ToolStreamEvent::ToolCallStart { index, id, name } => {
                let entry = pending_calls.entry(index).or_default();
                if id.is_some() {
                    entry.id = id;
                }
                if name.is_some() {
                    entry.name = name;
                }
            }
            ToolStreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                let entry = pending_calls.entry(index).or_default();
                entry.arguments_buffer.push_str(&arguments_delta);
            }
            ToolStreamEvent::ToolCallEnd {
                index,
                id,
                name,
                arguments,
            } => {
                let entry = pending_calls.entry(index).or_default();
                if id.is_some() {
                    entry.id = id;
                }
                if name.is_some() {
                    entry.name = name;
                }
                let arguments_copy = arguments.clone();
                let stable_call_id = normalized_tool_call_id(round, index, entry.id.clone());
                entry.id = Some(stable_call_id.clone());
                entry.final_arguments = Some(arguments);
                emit_stream_event(
                    stream_tx,
                    KernelStreamEventType::ToolCallStart,
                    KernelStreamEventData::ToolCallStart(ToolCallStartStreamData {
                        index,
                        call_id: Some(stable_call_id),
                        tool_name: entry.name.clone(),
                        arguments: arguments_copy,
                        round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::Done {
                finish_reason: final_reason,
                usage: final_usage,
            } => {
                finish_reason = final_reason;
                if let Some(final_usage) = final_usage {
                    usage = TalosUsage {
                        input_tokens: final_usage.prompt_tokens,
                        output_tokens: final_usage.completion_tokens,
                        total_tokens: final_usage.total_tokens,
                    };
                }
                break;
            }
        }
    }

    Ok(StreamRound {
        content,
        reasoning,
        tool_calls: finalize_tool_calls(round, pending_calls),
        finish_reason,
        saw_output_delta,
        usage,
    })
}

async fn chat_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
) -> TalosResult<StreamRound> {
    let response = llm_provider.chat_with_tools(request).await?;
    let tool_calls = response.get_tool_calls().to_vec();
    Ok(StreamRound {
        content: response.content,
        reasoning: String::new(),
        tool_calls,
        finish_reason: response.finish_reason,
        saw_output_delta: false,
        usage: TalosUsage {
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
            total_tokens: response.usage.total_tokens,
        },
    })
}

pub fn build_general_turn_step_request(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
) -> GeneralTurnStepRequest {
    GeneralTurnStepRequest {
        round: state.round,
        model: prepared.selected_model.clone(),
        messages: state.messages.clone(),
        tool_definitions: prepared.tool_definitions.clone(),
        parallel_tool_calls: true,
        native_thinking: None,
        stream_tx: prepared.stream_tx.clone(),
    }
}

pub async fn execute_general_turn_step(
    step: GeneralTurnStepRequest,
    llm_provider: Arc<dyn LlmProvider>,
) -> TalosResult<GeneralTurnStepResult> {
    let round = step.round;
    let stream_tx = step.stream_tx.clone();
    let round_response = if stream_tx.is_some() {
        stream_round(
            llm_provider,
            step.into_chat_request(),
            round,
            stream_tx.as_ref(),
        )
        .await?
    } else {
        chat_round(llm_provider, step.into_chat_request()).await?
    };

    let mut output_items = Vec::new();
    let mut reasoning = Vec::new();
    if !round_response.reasoning.trim().is_empty() {
        let reasoning_text = round_response.reasoning.trim().to_string();
        output_items.push(TalosOutputItem::Reasoning {
            text: reasoning_text.clone(),
        });
        reasoning.push(reasoning_text);
    }

    for call in &round_response.tool_calls {
        output_items.push(TalosOutputItem::ToolCall {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }

    let disposition = if round_response.tool_calls.is_empty() {
        GeneralTurnStepDisposition::FinalResponse
    } else {
        GeneralTurnStepDisposition::ContinueWithTools
    };
    let _finish_reason = ModelFinishReason::from_provider(
        round_response.finish_reason.as_deref(),
        !round_response.tool_calls.is_empty(),
    );

    Ok(GeneralTurnStepResult {
        text: round_response.content,
        reasoning,
        tool_calls: round_response.tool_calls,
        output_items,
        usage: round_response.usage,
        saw_output_delta: round_response.saw_output_delta,
        disposition,
    })
}

pub fn apply_general_turn_step(state: &mut GeneralTurnLoopState, step: &GeneralTurnStepResult) {
    state.saw_output_delta = state.saw_output_delta || step.saw_output_delta;
    state.usage.input_tokens = state
        .usage
        .input_tokens
        .saturating_add(step.usage.input_tokens);
    state.usage.output_tokens = state
        .usage
        .output_tokens
        .saturating_add(step.usage.output_tokens);
    state.usage.total_tokens = state
        .usage
        .total_tokens
        .saturating_add(step.usage.total_tokens);
    state.output_items.extend(step.output_items.clone());
    state.reasoning_log.extend(step.reasoning.clone());
    if !step.tool_calls.is_empty() {
        state.messages.push(ChatMessage::assistant_with_tool_calls(
            step.text.clone(),
            step.tool_calls.clone(),
        ));
    }
    for call in &step.tool_calls {
        state.tool_call_count += 1;
        state.tool_call_log.push(ToolCallRecord {
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }
}

fn build_tool_result_payload(
    tool_name: &str,
    outcome: &ToolDispatchOutcome,
) -> (Value, bool, Option<Value>) {
    match outcome {
        ToolDispatchOutcome::Pending { pending, .. } => match pending {
            ToolPending::Approval { prompt, arguments } => (
                serde_json::json!({
                    "ok": false,
                    "tool": tool_name,
                    "error": prompt,
                    "truncated": false,
                }),
                true,
                Some(arguments.clone()),
            ),
            ToolPending::Dynamic { arguments } => (
                serde_json::json!({
                    "ok": false,
                    "tool": tool_name,
                    "error": "dynamic tool response is pending",
                    "truncated": false,
                }),
                true,
                Some(arguments.clone()),
            ),
        },
        ToolDispatchOutcome::Executed { payload } => match payload {
            ToolOutputPayload::Function {
                content,
                success,
                structured_content,
            } => {
                let truncated = content.contains("[truncated]");
                if *success {
                    if let Some(structured) = structured_content {
                        (structured.clone(), false, None)
                    } else {
                        (
                            serde_json::json!({
                                "ok": true,
                                "tool": tool_name,
                                "result": content,
                                "truncated": truncated,
                            }),
                            false,
                            None,
                        )
                    }
                } else {
                    let error = structured_content
                        .as_ref()
                        .and_then(|value| value.get("error"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            if content.trim().is_empty() {
                                None
                            } else {
                                Some(content.clone())
                            }
                        })
                        .unwrap_or_else(|| "unknown error".to_string());
                    let mut value = serde_json::json!({
                        "ok": false,
                        "tool": tool_name,
                        "error": error,
                        "truncated": truncated,
                    });
                    if let Some(structured) = structured_content {
                        if let Some(object) = value.as_object_mut() {
                            object.insert("structured".to_string(), structured.clone());
                        }
                    }
                    (value, true, None)
                }
            }
            ToolOutputPayload::Resource {
                uri,
                mime_type,
                content,
            } => (
                serde_json::json!({
                    "ok": true,
                    "tool": tool_name,
                    "uri": uri,
                    "mime_type": mime_type,
                    "content": content,
                    "truncated": content.contains("[truncated]"),
                }),
                false,
                None,
            ),
            ToolOutputPayload::Mcp { content, is_error } => {
                if *is_error {
                    (
                        serde_json::json!({
                            "ok": false,
                            "tool": tool_name,
                            "error": content,
                            "truncated": content.contains("[truncated]"),
                        }),
                        true,
                        None,
                    )
                } else {
                    (
                        serde_json::json!({
                            "ok": true,
                            "tool": tool_name,
                            "result": content,
                            "truncated": content.contains("[truncated]"),
                        }),
                        false,
                        None,
                    )
                }
            }
        },
    }
}

pub async fn execute_general_turn_tool_batch(
    prepared: &PreparedGeneralTurn,
    tool_executor: Arc<dyn ToolExecutor>,
    tool_registry: Arc<dyn ToolExecutorPort>,
    kernel_config: Arc<AgentKernelConfig>,
    user_id: &str,
    tool_calls: &[FunctionCall],
    pre_approved_call_ids: std::collections::HashSet<String>,
    replay_dynamic_tool_results: std::collections::HashMap<String, Value>,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<GeneralTurnToolBatch> {
    let execution_context = ToolExecutionContext {
        session_id: format!("agent-{}", prepared.agent_id),
        user_id: user_id.to_string(),
        kernel_config,
        pre_approved_call_ids,
        dynamic_tool_results: replay_dynamic_tool_results,
    };
    let inputs = tool_calls
        .iter()
        .map(|call| ToolExecutionInput {
            call_id: call.id.clone(),
            tool_name: call.name.clone(),
            arguments: call.arguments.clone(),
        })
        .collect::<Vec<_>>();
    let outputs = tool_executor
        .execute(&execution_context, tool_registry, &inputs)
        .await?;

    let mut output_items = Vec::new();
    let mut tool_messages = Vec::new();
    let mut pending_confirmations = Vec::new();
    let mut pending_dynamic_requests = Vec::new();
    let mut retrieved_memories = Vec::new();
    let mut logs = Vec::new();
    let mut error_count = 0_usize;

    for output in outputs {
        let (payload, is_error, pending_arguments) =
            build_tool_result_payload(&output.tool_name, &output.outcome);
        if is_error {
            error_count += 1;
        }
        let memory_payload = output
            .outcome
            .payload()
            .and_then(|payload| payload.structured_content())
            .cloned()
            .unwrap_or_else(|| payload.clone());
        output_items.push(TalosOutputItem::ToolResult {
            call_id: output.call_id.clone(),
            tool_name: output.tool_name.clone(),
            content: vec![TalosContentItem::Json {
                value: payload.clone(),
            }],
            is_error,
        });
        emit_stream_event(
            stream_tx,
            KernelStreamEventType::ToolResult,
            KernelStreamEventData::ToolResult(ToolResultStreamData {
                call_id: output.call_id.clone(),
                tool_name: output.tool_name.clone(),
                content: vec![TalosContentItem::Json {
                    value: payload.clone(),
                }],
                is_error,
            }),
        )
        .await;
        logs.push(ToolResultLog {
            call_id: output.call_id.clone(),
            tool: output.tool_name.clone(),
            payload: payload.clone(),
            is_error,
            truncated: payload
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            size_bytes: serde_json::to_vec(&payload)
                .map(|buf| buf.len())
                .unwrap_or(0),
        });
        retrieved_memories.extend(parse_retrieved_memory_items(
            &output.tool_name,
            &memory_payload,
        ));
        if let ToolDispatchOutcome::Pending {
            pending: ToolPending::Approval { prompt, .. },
            ..
        } = &output.outcome
        {
            pending_confirmations.push(PendingToolConfirmation {
                tool_name: output.tool_name.clone(),
                call_id: output.call_id.clone(),
                prompt: prompt.clone(),
                arguments: pending_arguments.unwrap_or(Value::Null),
            });
        } else if let ToolDispatchOutcome::Pending {
            pending: ToolPending::Dynamic { .. },
            ..
        } = &output.outcome
        {
            pending_dynamic_requests.push(PendingDynamicToolRequest {
                tool_name: output.tool_name.clone(),
                call_id: output.call_id.clone(),
                arguments: pending_arguments.unwrap_or(Value::Null),
            });
        }
        tool_messages.push(ChatMessage::tool(
            output.call_id,
            output.tool_name,
            payload.to_string(),
        ));
    }

    Ok(GeneralTurnToolBatch {
        output_items,
        tool_messages,
        pending_confirmations,
        pending_dynamic_requests,
        retrieved_memories,
        logs,
        error_count,
    })
}

pub fn apply_general_turn_tool_batch(
    state: &mut GeneralTurnLoopState,
    batch: &GeneralTurnToolBatch,
    kernel_config: &AgentKernelConfig,
) {
    state.messages.extend(batch.tool_messages.clone());
    state.output_items.extend(batch.output_items.clone());
    state.tool_result_log.extend(batch.logs.clone());
    state.tool_error_count += batch.error_count;
    state
        .retrieved_memories
        .extend(batch.retrieved_memories.clone());

    if kernel_config.prompt.include_retrieved_memories {
        if let Some(message) = build_retrieved_memories_message(
            &state.retrieved_memories,
            kernel_config.prompt.retrieved_memory_max_items,
            kernel_config.prompt.retrieved_memory_max_chars,
        ) {
            if !state.messages.iter().rev().take(4).any(|item| {
                item.role == talos_llm_provider::Role::System && item.content == message.content
            }) {
                state.messages.push(message);
            }
        }
    }

    state.round += 1;
}

pub fn finalize_general_turn_artifacts(
    state: GeneralTurnLoopState,
    final_text: String,
    stop_reason: StopReason,
) -> ExecutedTurnArtifacts {
    let mut output_items = state.output_items;
    let mut reasoning_log = state.reasoning_log;
    let mut final_output = final_text;

    if final_output.trim().is_empty() {
        final_output = if state.tool_call_count > 0 {
            "Tool execution completed.".to_string()
        } else {
            "No response generated.".to_string()
        };
    }

    let execution_summary = build_execution_summary(state.tool_call_count, state.tool_error_count);
    output_items.push(TalosOutputItem::ExecutionSummary {
        text: execution_summary.clone(),
    });
    output_items.push(TalosOutputItem::OutputText {
        text: final_output.clone(),
    });
    reasoning_log.push(execution_summary);

    ExecutedTurnArtifacts {
        output_text: final_output,
        output_items,
        stop_reason,
        usage: state.usage,
        reasoning_log,
        tool_call_log: state.tool_call_log,
        tool_result_log: state.tool_result_log,
        saw_output_delta: state.saw_output_delta,
    }
}

pub async fn emit_general_turn_finalization_stream(
    prepared: &PreparedGeneralTurn,
    executed: &ExecutedTurnArtifacts,
) {
    if !executed.saw_output_delta && !executed.output_text.trim().is_empty() {
        emit_stream_event(
            prepared.stream_tx.as_ref(),
            KernelStreamEventType::OutputDelta,
            KernelStreamEventData::OutputDelta(OutputDeltaStreamData {
                delta: executed.output_text.clone(),
                round: None,
                source: Some(KernelOutputDeltaSource::KernelFallback),
            }),
        )
        .await;
    }

    if let Some(execution_summary) = executed.reasoning_log.last() {
        emit_stream_event(
            prepared.stream_tx.as_ref(),
            KernelStreamEventType::ExecutionSummary,
            KernelStreamEventData::ExecutionSummary(ExecutionSummaryStreamData {
                text: execution_summary.clone(),
                round: None,
            }),
        )
        .await;
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn prepare_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<PreparedGeneralTurn> {
    let session_id = ctx.session_id.clone();
    let invocation_id = ctx.invocation_id.clone();
    let agent_id = ctx.agent_id.clone();
    let selected_model = ctx.model.clone();
    let stream_tx = ctx.stream_tx.clone();

    let phase = match ctx.phase {
        crate::TurnPhase::Planning => PromptPhase::Planning,
        crate::TurnPhase::Execution => PromptPhase::Execution,
    };

    let prompt_builder = PromptBuilder::new()
        .agent(agent_name.to_string(), agent_description.to_string())
        .phase(phase)
        .planner_mode(kernel_config.planner_mode.as_str())?
        .planning_instruction(ctx.planning_instruction.clone())
        .config(PromptBuildConfig {
            budget: PromptBudget {
                max_static_tokens: kernel_config.prompt.max_static_tokens,
                max_dynamic_tokens: kernel_config.prompt.max_dynamic_tokens,
            },
            load_policy: PromptLoadPolicy {
                include_session_summary: kernel_config.prompt.include_session_summary,
                include_recent_messages: kernel_config.prompt.include_recent_messages,
                include_retrieved_memories: kernel_config.prompt.include_retrieved_memories,
                session_recent_limit: kernel_config.memory.session_recent_messages_limit,
                retrieved_memory_max_items: kernel_config.prompt.retrieved_memory_max_items,
                retrieved_memory_max_chars: kernel_config.prompt.retrieved_memory_max_chars,
                unified_recent_limit: 10,
            },
            prompt: kernel_config.prompt.clone(),
            memory: kernel_config.memory.clone(),
        })
        .tools(tool_definition_provider.as_ref())
        .memory_store(&session_id, memory_store.as_ref())
        .user_input(input.to_string());
    let prompt_assembly = prompt_builder.assemble().await?;
    let prompt_diagnostics = prompt_assembly.diagnostics.clone();
    let warnings = derive_prompt_warnings(&prompt_diagnostics, kernel_config);
    tracing::debug!(
        session_id = %session_id,
        agent_id = %agent_id,
        prompt_mode = ?prompt_diagnostics.mode,
        included_sections = ?prompt_diagnostics.included_sections,
        dropped_sections = ?prompt_diagnostics.dropped_sections,
        truncated_sections = ?prompt_diagnostics.truncated_sections,
        static_estimated_tokens = prompt_diagnostics.static_estimated_tokens,
        dynamic_estimated_tokens = prompt_diagnostics.dynamic_estimated_tokens,
        total_estimated_tokens = prompt_diagnostics.total_estimated_tokens,
        bootstrap_files = ?prompt_diagnostics.bootstrap_files,
        "prompt diagnostics"
    );
    if !warnings.is_empty() {
        warn!(
            session_id = %session_id,
            agent_id = %agent_id,
            warnings = ?warnings,
            "prompt warnings emitted"
        );
    }
    emit_prompt_warning_events(ctx, &warnings).await;

    Ok(PreparedGeneralTurn {
        session_id,
        invocation_id,
        agent_id,
        selected_model,
        prompt_prefix_messages: split_prompt_prefix_messages(
            &render_prompt_assembly(&prompt_assembly),
            input,
        ),
        messages: render_prompt_assembly(&prompt_assembly),
        tool_definitions: tool_definition_provider.get_tool_definitions(),
        prompt_diagnostics,
        warnings,
        stream_tx,
    })
}

pub async fn execute_general_turn_loop(
    prepared: PreparedGeneralTurn,
    agent_name: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<ExecutedTurnArtifacts> {
    let max_tool_rounds = kernel_config.max_tool_rounds.max(1);
    let mut state = GeneralTurnLoopState::new(&prepared);
    let execution_settings =
        crate::KernelExecutionSettings::try_from(kernel_config).unwrap_or_default();
    let tool_executor = crate::ToolExecutorFactory::build(execution_settings.tool_execution_mode);
    let kernel_config = Arc::new(kernel_config.clone());

    let (final_text, stop_reason) = loop {
        let step_request = build_general_turn_step_request(&prepared, &state);
        let step = execute_general_turn_step(step_request, llm_provider.clone()).await?;
        let step_text = step.text.clone();
        let step_disposition = step.disposition;
        apply_general_turn_step(&mut state, &step);

        if matches!(step_disposition, GeneralTurnStepDisposition::FinalResponse) {
            state
                .messages
                .push(ChatMessage::assistant(step_text.clone()));
            break (step_text, StopReason::EndTurn);
        }

        let tool_batch = execute_general_turn_tool_batch(
            &prepared,
            tool_executor.clone(),
            tool_executor_port.clone(),
            kernel_config.clone(),
            agent_name,
            &step.tool_calls,
            std::collections::HashSet::new(),
            std::collections::HashMap::new(),
            prepared.stream_tx.as_ref(),
        )
        .await?;
        apply_general_turn_tool_batch(&mut state, &tool_batch, kernel_config.as_ref());

        if !tool_batch.pending_confirmations.is_empty()
            || !tool_batch.pending_dynamic_requests.is_empty()
        {
            break (
                if !tool_batch.pending_confirmations.is_empty() {
                    "Tool confirmation pending.".to_string()
                } else {
                    "Dynamic tool response pending.".to_string()
                },
                StopReason::EndTurn,
            );
        }

        if state.round >= max_tool_rounds {
            break (
                "[tool-call rounds exceeded]".to_string(),
                StopReason::ToolRoundLimit,
            );
        }
    };

    let executed = finalize_general_turn_artifacts(state, final_text, stop_reason);
    emit_general_turn_finalization_stream(&prepared, &executed).await;
    Ok(executed)
}

#[allow(clippy::too_many_arguments)]
pub async fn persist_general_turn_artifacts(
    prepared: &PreparedGeneralTurn,
    input: &str,
    executed: &ExecutedTurnArtifacts,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) {
    let seq = match memory_store.next_turn_seq(&prepared.session_id).await {
        Ok(seq) => seq,
        Err(err) => {
            warn!(
                session_id = %prepared.session_id,
                error = %err,
                "failed to allocate next turn seq"
            );
            return;
        }
    };
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let talos_turn = TalosTurn {
        turn_id: turn_id.clone(),
        session_id: prepared.session_id.to_string(),
        seq,
        timestamp: now,
        messages: vec![
            TalosMessage {
                header: RecordHeader {
                    id: format!("{}:u", turn_id),
                    session_id: prepared.session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                },
                role: TalosMessageRole::User,
                text: input.to_string(),
            },
            TalosMessage {
                header: RecordHeader {
                    id: format!("{}:a", turn_id),
                    session_id: prepared.session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                },
                role: TalosMessageRole::Assistant,
                text: executed.output_text.clone(),
            },
        ],
        reasoning: executed.reasoning_log.clone(),
        tool_calls: executed.tool_call_log.clone(),
        tool_results: executed
            .tool_result_log
            .iter()
            .map(|item| ToolResultRecord {
                header: RecordHeader {
                    id: format!("{}:tool:{}", turn_id, item.call_id),
                    session_id: prepared.session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                },
                call_id: item.call_id.clone(),
                tool: item.tool.clone(),
                payload: item.payload.clone(),
                is_error: item.is_error,
                truncated: item.truncated,
                size_bytes: item.size_bytes,
            })
            .collect(),
    };
    if let Err(err) = memory_store.append_turn(&talos_turn).await {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to append memory store turn"
        );
    }

    if let Err(err) = compact_workspace_context_if_needed(
        &prepared.session_id,
        &prepared.selected_model,
        llm_provider.clone(),
        memory_store.as_ref(),
        kernel_config,
    )
    .await
    {
        warn!(
            session_id = %prepared.session_id,
            error = %err,
            "failed to compact workspace memory context"
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn run_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<GeneralTurnResult> {
    let prepared = prepare_general_turn(
        ctx,
        agent_name,
        agent_description,
        input,
        tool_definition_provider,
        memory_store.clone(),
        kernel_config,
    )
    .await?;
    let prompt_diagnostics = prepared.prompt_diagnostics.clone();
    let warnings = prepared.warnings.clone();
    let executed = execute_general_turn_loop(
        prepared.clone(),
        agent_name,
        llm_provider.clone(),
        tool_executor_port,
        kernel_config,
    )
    .await?;
    persist_general_turn_artifacts(
        &prepared,
        input,
        &executed,
        llm_provider.clone(),
        memory_store,
        kernel_config,
    )
    .await;

    Ok(GeneralTurnResult {
        output_text: executed.output_text,
        output_items: executed.output_items,
        stop_reason: executed.stop_reason,
        usage: executed.usage,
        prompt_diagnostics,
        warnings,
    })
}
