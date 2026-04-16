use super::*;
use crate::{
    ContextStateBags, InvocationExecutionState, InvocationLimits, KernelEventData,
    KernelStreamEventData, KernelStreamEventType, ToolExecutionMode, TurnPhase,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, ToolStream,
    ToolStreamEvent, Usage,
};
use talos_tools::{register_core_local_tools, ToolRegistry};
use tokio::sync::mpsc;
use uuid::Uuid;

mod loop_tests;
mod memory_summary_tests;
mod parsing_tests;
mod stream_tests;

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

struct StreamingToolExecutionProvider {
    path: String,
}

#[derive(Default)]
struct EmptyFinalAfterToolsProvider {
    round: AtomicUsize,
    dir_path: String,
}

#[derive(Default)]
struct SalvageAfterEmptyFinalProvider {
    round: AtomicUsize,
    dir_path: String,
}

struct SummaryProvider {
    responses: Mutex<VecDeque<String>>,
}

impl SummaryProvider {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
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
impl LlmProvider for StreamingToolExecutionProvider {
    fn name(&self) -> &str {
        "streaming-tool-execution"
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
        let path = self.path.clone();
        tokio::spawn(async move {
            tx.send(Ok(ToolStreamEvent::TextDelta {
                delta: "Inspecting a file.".to_string(),
            }))
            .await
            .expect("text delta should send");
            tx.send(Ok(ToolStreamEvent::ToolCallStart {
                index: 0,
                id: Some("call_read".to_string()),
                name: Some("read_file".to_string()),
            }))
            .await
            .expect("tool start should send");
            tx.send(Ok(ToolStreamEvent::ToolCallDelta {
                index: 0,
                arguments_delta: format!("{{\"path\":\"{}\"}}", path),
            }))
            .await
            .expect("tool delta should send");
            tx.send(Ok(ToolStreamEvent::ToolCallEnd {
                index: 0,
                id: Some("call_read".to_string()),
                name: Some("read_file".to_string()),
                arguments: json!({ "path": path }),
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
        "streaming-tool-execution"
    }
}

#[async_trait]
impl LlmProvider for EmptyFinalAfterToolsProvider {
    fn name(&self) -> &str {
        "empty-final-after-tools"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        match self.round.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ChatResponse {
                content: "Inspect the project root first.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_list",
                    "list_dir",
                    json!({
                        "dir_path": self.dir_path,
                        "depth": 1
                    }),
                )]),
            }),
            1 => Ok(ChatResponse {
                content: String::new(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
            _ => Ok(ChatResponse {
                content: "Final architecture summary based on verified tool results.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> &str {
        "empty-final-after-tools"
    }
}

#[async_trait]
impl LlmProvider for SalvageAfterEmptyFinalProvider {
    fn name(&self) -> &str {
        "salvage-after-empty-final"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        match self.round.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ChatResponse {
                content: "Inspect the project root first.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_list",
                    "list_dir",
                    json!({
                        "dir_path": self.dir_path,
                        "depth": 1
                    }),
                )]),
            }),
            1 | 2 => Ok(ChatResponse {
                content: String::new(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
            _ => Ok(ChatResponse {
                content: "Salvaged final answer from verified tool results.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> &str {
        "salvage-after-empty-final"
    }
}

#[async_trait]
impl LlmProvider for SummaryProvider {
    fn name(&self) -> &str {
        "summary-provider"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        let response = self
            .responses
            .lock()
            .expect("summary provider mutex should lock")
            .pop_front()
            .expect("summary provider should have a queued response");
        Ok(ChatResponse {
            content: response,
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
        project_root: None,
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
