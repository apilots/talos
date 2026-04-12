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

struct SummaryProvider {
    response: String,
}

#[async_trait]
impl LlmProvider for NoopLlmProvider {
    fn name(&self) -> &str {
        "noop"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "ok".to_string(),
            reasoning: Vec::new(),
            model: "noop".to_string(),
            usage: Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
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
                reasoning: Vec::new(),
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
                assert!(payload.get("output").and_then(Value::as_str).is_some());
                Ok(ChatResponse {
                    content: "Write stdin to finish.".to_string(),
                    reasoning: Vec::new(),
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
                    reasoning: Vec::new(),
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
                reasoning: Vec::new(),
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

#[async_trait]
impl LlmProvider for SummaryProvider {
    fn name(&self) -> &str {
        "summary-provider"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: self.response.clone(),
            reasoning: Vec::new(),
            model: "summary-model".to_string(),
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> &str {
        "summary-model"
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
        selected_model_info: talos_types::ModelInfo {
            id: "m1".to_string(),
            name: "m1".to_string(),
            provider: "p1".to_string(),
            context_window: 0,
            max_tokens: 0,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        },
        phase: TurnPhase::Execution,
        planning_instruction: None,
        transfer_to: None,
        execution_state: InvocationExecutionState::default(),
        session: crate::InvocationSession {
            session_id: "s1".to_string(),
            ..crate::InvocationSession::default()
        },
        session_environment: None,
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
            items: vec![TalosOutputItem::UserInput {
                text: "history that should be dropped".to_string(),
            }],
            usage: TalosUsage::default(),
            stop_reason: TalosApiStopReason::EndTurn,
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
            start_line: 1,
            end_line: 1,
        }],
    });

    let items = parse_retrieved_memory_items("memory_search", &payload);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, "memory/2026-03-24.md");
}

#[test]
fn test_parse_retrieved_memory_items_ignores_invalid_search_rows() {
    let payload = json!({
        "results": [
            {
                "path": "memory/2026-03-25.md",
                "snippet": "valid snippet",
                "start_line": 1,
                "end_line": 1
            }
        ]
    });

    let items = parse_retrieved_memory_items("memory_search", &payload);
    assert_eq!(items.len(), 1);
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

#[tokio::test]
async fn test_flush_session_memory_rejects_blank_summary_title() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let result = flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider {
            response: "{\"title\":\"   \",\"facts\":[\"remember tea\"]}".to_string(),
        }),
        &memory_store,
        &config,
        false,
        false,
        true,
    )
    .await;

    assert!(matches!(result, Err(talos_error::TalosError::ToolError(_))));
    assert!(
        !workspace_dir.join("memory").exists(),
        "blank summary title should not persist workspace memory"
    );
}

#[tokio::test]
async fn test_flush_session_memory_returns_early_when_threshold_is_zero() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.memory.compaction_threshold_bytes = 0;

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider {
            response: "{\"title\":\"Durable Memory\",\"facts\":[\"remember tea\"]}".to_string(),
        }),
        &memory_store,
        &config,
        true,
        true,
        true,
    )
    .await
    .expect("threshold zero should skip without error");

    assert!(
        !workspace_dir.join("memory").exists(),
        "threshold zero should skip workspace writes"
    );
    assert!(
        memory_store
            .load_summaries("s1", 1)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "threshold zero should skip session summary writes"
    );
}

#[tokio::test]
async fn test_flush_session_memory_returns_early_when_transcript_is_blank() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    let timestamp = Utc::now();
    memory_store
        .append_turn(&TalosTurn {
            turn_id: "turn-blank".to_string(),
            session_id: "s1".to_string(),
            seq: memory_store
                .next_turn_seq("s1")
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![TalosOutputItem::UserInput {
                text: "   ".to_string(),
            }],
            usage: TalosUsage::default(),
            stop_reason: TalosApiStopReason::EndTurn,
        })
        .await
        .expect("append should succeed");

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider {
            response: "{\"title\":\"Durable Memory\",\"facts\":[\"remember tea\"]}".to_string(),
        }),
        &memory_store,
        &config,
        false,
        true,
        true,
    )
    .await
    .expect("blank transcript should skip without error");

    assert!(
        !workspace_dir.join("memory").exists(),
        "blank transcript should not persist workspace writes"
    );
    assert!(
        memory_store
            .load_summaries("s1", 1)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "blank transcript should not persist session summary"
    );
}
