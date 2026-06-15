use super::*;
use crate::{
    ContextStateBags, InvocationExecutionState, InvocationLimits, KernelEventData,
    KernelStreamEventData, KernelStreamEventType, ToolExecutionMode, ToolProgressKind, TurnPhase,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider,
    ModelBlockCompleted, ModelBlockDelta, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, Usage,
};
use talos_tools::{register_core_local_tools, ToolRegistry};
use tokio::sync::mpsc;
use uuid::Uuid;

mod compaction_loop_tests;
mod final_response_tests;
mod memory_summary_tests;
mod parsing_tests;
mod prompt_warning_tests;
mod step_request_tests;
mod stream_tests;
mod tool_loop_tests;

struct NoopLlmProvider;

#[derive(Default)]
struct ExecLoopProvider {
    round: AtomicUsize,
}

struct StreamingToolCallIdProvider;

struct StreamingToolExecutionProvider {
    path: String,
}

struct LengthFinishProvider;

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

struct RecordingSummaryProvider {
    responses: Mutex<VecDeque<String>>,
    user_prompts: Mutex<Vec<String>>,
}

impl SummaryProvider {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }
}

impl RecordingSummaryProvider {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            user_prompts: Mutex::new(Vec::new()),
        }
    }

    fn recorded_user_prompts(&self) -> Vec<String> {
        self.user_prompts
            .lock()
            .expect("recording summary provider mutex should lock")
            .clone()
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
        let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        tx.send(Ok(ChatChunk {
            delta: "ok".to_string(),
            finish_reason: Some("stop".to_string()),
        }))
        .await
        .expect("noop stream chunk should send");
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(4);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(None, None),
            )))
            .await
            .expect("assistant start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "ok".to_string(),
                },
            }))
            .await
            .expect("assistant delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("assistant completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: Some(Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            }))
            .await
            .expect("done should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "noop".to_string()
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
                content: "Start a background bash task first.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_bash",
                    "Bash",
                    json!({
                        "command": "sleep 0.2; printf 'done:ok\\n'",
                        "description": "Run a short background task for verification.",
                        "run_in_background": true,
                        "timeout": 50
                    }),
                )]),
            }),
            1 => {
                let tool_message = request
                    .messages
                    .iter()
                    .find(|msg| {
                        msg.role == talos_llm_provider::Role::Tool
                            && msg.tool_call_id.as_deref() == Some("call_bash")
                    })
                    .expect("Bash tool message should exist");
                let payload: Value =
                    serde_json::from_str(&tool_message.content).expect("Bash payload should parse");
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
                    content: "Check the background task output.".to_string(),
                    reasoning: Vec::new(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("tool_calls".to_string()),
                    tool_calls: Some(vec![FunctionCall::new(
                        "call_output",
                        "TaskOutput",
                        json!({
                            "task_id": session_id,
                            "offset": 0,
                            "max_output_tokens": 2000
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
                            && msg.tool_call_id.as_deref() == Some("call_output")
                    })
                    .expect("TaskOutput tool message should exist");
                let payload: Value = serde_json::from_str(&tool_message.content)
                    .expect("TaskOutput payload should parse");
                let output = payload
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Ok(ChatResponse {
                    content: format!("Final answer after task output: {}", output.trim()),
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

    fn default_model(&self) -> String {
        "exec-loop".to_string()
    }
}

#[async_trait]
impl LlmProvider for LengthFinishProvider {
    fn name(&self) -> &str {
        "length-finish"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "partial answer".to_string(),
            reasoning: Vec::new(),
            model: "length-finish".to_string(),
            usage: Usage {
                prompt_tokens: 1,
                completion_tokens: 4,
                total_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            finish_reason: Some("length".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "length-finish".to_string()
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

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(8);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::tool_call(
                    0,
                    Some(String::new()),
                    Some("MemorySearch".to_string()),
                ),
            )))
            .await
            .expect("tool start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: json!({ "query": "talos项目" }).to_string(),
                },
            }))
            .await
            .expect("tool args should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "tool_call_0".to_string(),
                },
            )))
            .await
            .expect("tool completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("tool_calls".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "streaming-tool-call-id".to_string()
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

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(8);
        let path = self.path.clone();
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(None, None),
            )))
            .await
            .expect("assistant start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Inspecting a file.".to_string(),
                },
            }))
            .await
            .expect("text delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("assistant completed should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::tool_call(
                    0,
                    Some("call_read".to_string()),
                    Some("read".to_string()),
                ),
            )))
            .await
            .expect("tool start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: format!("{{\"path\":\"{}\"}}", path),
                },
            }))
            .await
            .expect("tool delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "tool_call_0".to_string(),
                },
            )))
            .await
            .expect("tool completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("tool_calls".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "streaming-tool-execution".to_string()
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
                    "Glob",
                    json!({
                        "path": self.dir_path,
                        "pattern": "**/*",
                        "limit": 50
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

    fn default_model(&self) -> String {
        "empty-final-after-tools".to_string()
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
                    "Glob",
                    json!({
                        "path": self.dir_path,
                        "pattern": "**/*",
                        "limit": 50
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

    fn default_model(&self) -> String {
        "salvage-after-empty-final".to_string()
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
            usage: Usage {
                prompt_tokens: 13,
                completion_tokens: 7,
                total_tokens: 20,
                cache_creation_input_tokens: Some(3),
                cache_read_input_tokens: Some(11),
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "summary-model".to_string()
    }
}

#[async_trait]
impl LlmProvider for RecordingSummaryProvider {
    fn name(&self) -> &str {
        "recording-summary-provider"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        if let Some(user_prompt) = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == talos_llm_provider::Role::User)
            .map(|message| message.content.clone())
        {
            self.user_prompts
                .lock()
                .expect("recording summary provider prompt mutex should lock")
                .push(user_prompt);
        }

        let response = self
            .responses
            .lock()
            .expect("recording summary provider mutex should lock")
            .pop_front()
            .expect("recording summary provider should have a queued response");
        Ok(ChatResponse {
            content: response,
            reasoning: Vec::new(),
            model: "summary-model".to_string(),
            usage: Usage {
                prompt_tokens: 13,
                completion_tokens: 7,
                total_tokens: 20,
                cache_creation_input_tokens: Some(3),
                cache_read_input_tokens: Some(11),
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "summary-model".to_string()
    }
}

fn build_ctx_with_stream(
    stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
) -> InvocationContext {
    InvocationContext {
        session_id: "s1".to_string(),
        message_id: "msg-1".to_string(),
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
        transcript_service: None,
        stream_tx,
    }
}

async fn seed_recent_user_message(memory_store: &MemoryStore) {
    let timestamp = Utc::now();
    memory_store
        .append_turn(&TalosTurn {
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
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
