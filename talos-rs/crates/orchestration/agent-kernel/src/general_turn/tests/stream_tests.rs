use super::*;
use talos_llm_provider::{
    ModelBlockCompleted, ModelBlockDelta, ModelBlockKind, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, ModelOutputPhase,
};

struct DirectStreamingAnswerProvider;
struct ExplicitFinalModelBlockProvider;
struct ScriptedModelBlockProvider {
    name: &'static str,
    events: Vec<ModelBlockStreamEvent>,
}
#[derive(Default)]
struct DirectTerminalStreamingProvider {
    calls: AtomicUsize,
}

impl ScriptedModelBlockProvider {
    fn new(name: &'static str, events: Vec<ModelBlockStreamEvent>) -> Self {
        Self { name, events }
    }
}

#[async_trait]
impl LlmProvider for ScriptedModelBlockProvider {
    fn name(&self) -> &str {
        self.name
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in scripted model block tests");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(16);
        let events = self.events.clone();
        tokio::spawn(async move {
            for event in events {
                tx.send(Ok(event))
                    .await
                    .expect("scripted event should send");
            }
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        self.name.to_string()
    }
}

fn assistant_started(model_item_id: &str, output_phase: ModelOutputPhase) -> ModelBlockStreamEvent {
    ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
        model_item_id: model_item_id.to_string(),
        provider_item_id: None,
        kind: ModelBlockKind::AssistantText,
        output_phase,
        tool_call: None,
    })
}

fn reasoning_started(model_item_id: &str) -> ModelBlockStreamEvent {
    ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
        model_item_id: model_item_id.to_string(),
        provider_item_id: None,
        kind: ModelBlockKind::Reasoning,
        output_phase: ModelOutputPhase::Unknown,
        tool_call: None,
    })
}

fn block_completed(model_item_id: &str) -> ModelBlockStreamEvent {
    ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
        model_item_id: model_item_id.to_string(),
    })
}

fn reasoning_completed(text: &str) -> ModelBlockStreamEvent {
    ModelBlockStreamEvent::ReasoningCompleted {
        output: ReasoningOutput {
            display_text: vec![text.to_string()],
            continuation: Some(ReasoningContinuation::Text(text.to_string())),
        },
    }
}

fn done() -> ModelBlockStreamEvent {
    ModelBlockStreamEvent::Done {
        finish_reason: Some("stop".to_string()),
        usage: None,
    }
}

#[async_trait]
impl LlmProvider for DirectStreamingAnswerProvider {
    fn name(&self) -> &str {
        "direct-streaming-answer"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in streaming step test");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(4);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(None, ModelOutputPhase::Unknown),
            )))
            .await
            .expect("started event should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Direct answer.".to_string(),
                },
            }))
            .await
            .expect("text delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                talos_llm_provider::ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("completed event should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "direct-streaming-answer".to_string()
    }
}

#[async_trait]
impl LlmProvider for ExplicitFinalModelBlockProvider {
    fn name(&self) -> &str {
        "explicit-final-model-block"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in streaming step test");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(4);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(
                    Some("provider-final-1".to_string()),
                    ModelOutputPhase::FinalAnswer,
                ),
            )))
            .await
            .expect("started event should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Explicit final.".to_string(),
                },
            }))
            .await
            .expect("delta event should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                talos_llm_provider::ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("completed event should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "explicit-final-model-block".to_string()
    }
}

#[async_trait]
impl LlmProvider for DirectTerminalStreamingProvider {
    fn name(&self) -> &str {
        "direct-loop-terminal-streaming"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in streaming loop test");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        let text = if call_index == 0 {
            "Working answer."
        } else {
            "Final summary."
        };
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(4);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(None, ModelOutputPhase::Unknown),
            )))
            .await
            .expect("started event should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: text.to_string(),
                },
            }))
            .await
            .expect("delta event should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                talos_llm_provider::ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("completed event should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "direct-loop-terminal-streaming".to_string()
    }
}

#[tokio::test]
async fn assistant_delta_without_block_fails() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "assistant-delta-without-block",
        vec![ModelBlockStreamEvent::BlockDelta {
            model_item_id: "missing-assistant".to_string(),
            delta: ModelBlockDelta::AssistantText {
                delta: "orphan".to_string(),
            },
        }],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect_err("orphan assistant delta should fail");

    assert!(err
        .to_string()
        .contains("assistant text delta received without active assistant block"));
}

#[tokio::test]
async fn reasoning_delta_without_block_fails() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "reasoning-delta-without-block",
        vec![ModelBlockStreamEvent::BlockDelta {
            model_item_id: "missing-reasoning".to_string(),
            delta: ModelBlockDelta::ReasoningText {
                delta: "think".to_string(),
            },
        }],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect_err("orphan reasoning delta should fail");

    assert!(err
        .to_string()
        .contains("reasoning delta received without active reasoning block"));
}

#[tokio::test]
async fn tool_args_delta_without_block_fails() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "tool-args-without-block",
        vec![ModelBlockStreamEvent::BlockDelta {
            model_item_id: "missing-tool".to_string(),
            delta: ModelBlockDelta::ToolArguments {
                delta: "{}".to_string(),
            },
        }],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect_err("orphan tool args should fail");

    assert!(err
        .to_string()
        .contains("tool arguments delta received without active tool block"));
}

#[tokio::test]
async fn assistant_delta_with_wrong_model_item_id_fails() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "assistant-wrong-id",
        vec![
            assistant_started("assistant-1", ModelOutputPhase::Unknown),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant-2".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "wrong".to_string(),
                },
            },
        ],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect_err("wrong assistant id should fail");

    assert!(err
        .to_string()
        .contains("assistant text delta received without active assistant block assistant-2"));
}

#[tokio::test]
async fn multiple_assistant_blocks_keep_separate_lifecycle() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "multiple-assistant-blocks",
        vec![
            assistant_started("assistant-1", ModelOutputPhase::Unknown),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant-1".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "first ".to_string(),
                },
            },
            block_completed("assistant-1"),
            assistant_started("assistant-2", ModelOutputPhase::Unknown),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant-2".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "second".to_string(),
                },
            },
            block_completed("assistant-2"),
            done(),
        ],
    ));

    let result = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect("multiple assistant blocks should stream");

    assert_eq!(result.content, "first second");
}

#[tokio::test]
async fn final_answer_then_tool_call_fails_in_kernel() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "final-then-tool",
        vec![
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final".to_string(),
                },
            },
            block_completed("final"),
            ModelBlockStreamEvent::BlockStarted(ModelBlockStarted::tool_call(
                0,
                Some("call_after_final".to_string()),
                Some("read".to_string()),
            )),
        ],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        None,
        None,
    )
    .await
    .expect_err("tool call after final answer should fail");

    assert!(err
        .to_string()
        .contains("tool call block received after final answer started"));
}

#[tokio::test]
async fn final_answer_then_commentary_fails_in_kernel() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "final-then-commentary",
        vec![
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final".to_string(),
                },
            },
            block_completed("final"),
            assistant_started("commentary", ModelOutputPhase::Unknown),
        ],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        None,
        None,
    )
    .await
    .expect_err("commentary after final answer should fail");

    assert!(err
        .to_string()
        .contains("unknown assistant text received after final answer started"));
}

#[tokio::test]
async fn final_answer_then_tool_args_fails_in_kernel() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "final-then-tool-args",
        vec![
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final".to_string(),
                },
            },
            block_completed("final"),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: "{}".to_string(),
                },
            },
        ],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        None,
        None,
    )
    .await
    .expect_err("tool args after final answer should fail");

    assert!(err
        .to_string()
        .contains("tool arguments delta received after final answer started"));
}

#[tokio::test]
async fn final_answer_then_reasoning_fails_in_kernel() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "final-then-reasoning",
        vec![
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final".to_string(),
                },
            },
            block_completed("final"),
            reasoning_started("late-reasoning"),
        ],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        None,
        None,
    )
    .await
    .expect_err("reasoning after final answer should fail");

    assert!(err
        .to_string()
        .contains("reasoning block received after final answer started"));
}

#[tokio::test]
async fn explicit_final_after_reasoning_closes_reasoning_before_summary() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "reasoning-then-final",
        vec![
            reasoning_started("reasoning-1"),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "reasoning-1".to_string(),
                delta: ModelBlockDelta::ReasoningText {
                    delta: "thinking".to_string(),
                },
            },
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final answer.".to_string(),
                },
            },
            block_completed("final"),
            reasoning_completed("thinking"),
            done(),
        ],
    ));
    let (tx, mut rx) = mpsc::channel(16);

    stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        Some(&tx),
        None,
    )
    .await
    .expect("explicit final after reasoning should stream");

    let mut event_types = Vec::new();
    while let Ok(event) = rx.try_recv() {
        event_types.push(event.event_type);
    }
    let final_started = event_types
        .iter()
        .rposition(|kind| matches!(kind, KernelStreamEventType::AssistantTextStarted))
        .expect("final should start");

    assert!(event_types[..final_started]
        .iter()
        .all(|kind| !matches!(kind, KernelStreamEventType::ExecutionSummary)));
}

#[tokio::test]
async fn unknown_model_block_completion_fails() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "unknown-completion",
        vec![block_completed("missing-block"), done()],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect_err("unknown block completion should fail");

    assert!(err
        .to_string()
        .contains("model block completed without active block missing-block"));
}

#[tokio::test]
async fn repeated_completion_for_known_closed_reasoning_is_ignored() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "repeated-known-completion",
        vec![
            reasoning_started("reasoning-1"),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "reasoning-1".to_string(),
                delta: ModelBlockDelta::ReasoningText {
                    delta: "thinking live".to_string(),
                },
            },
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            block_completed("reasoning-1"),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final answer.".to_string(),
                },
            },
            block_completed("final"),
            reasoning_completed("thinking live"),
            done(),
        ],
    ));
    let (tx, mut rx) = mpsc::channel(16);

    stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        Some(&tx),
        None,
    )
    .await
    .expect("duplicate completion for known closed block should be ignored");

    let mut saw_thinking_delta = false;
    let mut saw_final_delta = false;
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::ThinkingDelta(data) => {
                if data.delta == "thinking live" {
                    saw_thinking_delta = true;
                }
            }
            KernelStreamEventData::AssistantTextDelta(data) if data.delta == "Final answer." => {
                saw_final_delta = true;
            }
            _ => {}
        }
    }

    assert!(saw_thinking_delta);
    assert!(saw_final_delta);
}

#[tokio::test]
async fn explicit_final_with_pending_tool_args_fails_by_policy() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "pending-tool-then-final",
        vec![
            ModelBlockStreamEvent::BlockStarted(ModelBlockStarted::tool_call(
                0,
                Some("call_pending".to_string()),
                Some("read".to_string()),
            )),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: "{\"path\":\"/tmp/a\"}".to_string(),
                },
            },
            assistant_started("final", ModelOutputPhase::FinalAnswer),
        ],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        None,
        None,
    )
    .await
    .expect_err("final with active tool args should fail");

    assert!(err
        .to_string()
        .contains("final answer started after tool call was emitted in same round"));
}

#[tokio::test]
async fn explicit_final_closes_active_commentary_before_summary() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "commentary-then-final",
        vec![
            assistant_started("commentary", ModelOutputPhase::Unknown),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "commentary".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Working note.".to_string(),
                },
            },
            assistant_started("final", ModelOutputPhase::FinalAnswer),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "final".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Final answer.".to_string(),
                },
            },
            block_completed("final"),
            done(),
        ],
    ));
    let (tx, mut rx) = mpsc::channel(16);

    stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        Some("Summary".to_string()),
        Some(&tx),
        None,
    )
    .await
    .expect("explicit final should stream");

    let mut event_types = Vec::new();
    while let Ok(event) = rx.try_recv() {
        event_types.push(event.event_type);
    }
    let final_started = event_types
        .iter()
        .rposition(|kind| matches!(kind, KernelStreamEventType::AssistantTextStarted))
        .expect("final should start");

    assert!(event_types[..final_started]
        .iter()
        .all(|kind| !matches!(kind, KernelStreamEventType::ExecutionSummary)));
}

#[tokio::test]
async fn tool_progress_delta_maps_to_kernel_tool_progress() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "tool-progress",
        vec![
            ModelBlockStreamEvent::BlockStarted(ModelBlockStarted::tool_call(
                0,
                Some("call_progress".to_string()),
                Some("read".to_string()),
            )),
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolProgress {
                    delta: "reading file".to_string(),
                },
            },
            ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: "{\"path\":\"/tmp/a\"}".to_string(),
                },
            },
            block_completed("tool_call_0"),
            ModelBlockStreamEvent::Done {
                finish_reason: Some("tool_calls".to_string()),
                usage: None,
            },
        ],
    ));
    let (tx, mut rx) = mpsc::channel(16);

    stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        Some(&tx),
        None,
    )
    .await
    .expect("tool progress should stream");

    let mut progress = None;
    while let Ok(event) = rx.try_recv() {
        if let KernelStreamEventData::ToolProgressDelta(data) = event.data {
            progress = Some(data);
            break;
        }
    }
    let progress = progress.expect("tool progress delta should be emitted");
    assert_eq!(progress.call_id, "call_progress");
    assert_eq!(progress.tool_name, "read");
    assert_eq!(progress.message, "reading file");
    assert_eq!(progress.progress_kind, ToolProgressKind::Status);
}

#[tokio::test]
async fn tool_progress_without_active_tool_fails() {
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "tool-progress-without-block",
        vec![ModelBlockStreamEvent::BlockDelta {
            model_item_id: "missing-tool".to_string(),
            delta: ModelBlockDelta::ToolProgress {
                delta: "orphan progress".to_string(),
            },
        }],
    ));

    let err = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        None,
    )
    .await
    .expect_err("orphan tool progress should fail");

    assert!(err
        .to_string()
        .contains("tool progress delta received without active tool block"));
}

#[tokio::test]
async fn test_stream_round_normalizes_empty_tool_call_id_before_emitting_start_event() {
    let provider = Arc::new(StreamingToolCallIdProvider);
    let request = ChatRequest::new_user("Search workspace memory", "streaming-tool-call-id");
    let (tx, mut rx) = mpsc::channel(8);

    let result = stream_round(
        provider,
        request,
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        Some(&tx),
        None,
    )
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
async fn streamed_tool_blocks_execute_in_model_index_order() {
    let question_arguments = serde_json::json!({
        "header": "Order",
        "question": "Which option?",
        "options": [
            {"id": "first", "label": "First", "description": "Choose first"},
            {"id": "second", "label": "Second", "description": "Choose second"}
        ]
    })
    .to_string();
    let mut events = Vec::new();
    for index in 0..3 {
        events.push(ModelBlockStreamEvent::BlockStarted(
            ModelBlockStarted::tool_call(
                index,
                Some(format!("call-{index}")),
                Some("AskUserQuestion".to_string()),
            ),
        ));
        events.push(ModelBlockStreamEvent::BlockDelta {
            model_item_id: format!("tool_call_{index}"),
            delta: ModelBlockDelta::ToolArguments {
                delta: question_arguments.clone(),
            },
        });
    }
    for index in [2, 1, 0] {
        events.push(block_completed(&format!("tool_call_{index}")));
    }
    events.push(ModelBlockStreamEvent::Done {
        finish_reason: Some("tool_calls".to_string()),
        usage: None,
    });
    let provider = Arc::new(ScriptedModelBlockProvider::new(
        "reverse-tool-completion",
        events,
    ));
    let registry = Arc::new(ToolRegistry::new());
    talos_tools::register_core_local_tools(registry.as_ref());
    let config = Arc::new(AgentKernelConfig::default());

    let result = stream_round(
        provider,
        ChatRequest::new_user("test", "test-model"),
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        None,
        Some(StreamingToolExecutionConfig {
            execution_mode: ToolExecutionMode::Sequential,
            max_parallelism: 1,
            execution_context: ToolExecutionContext {
                session_id: "ordered-stream".to_string(),
                turn_id: "turn-ordered-stream".to_string(),
                agent_id: "agent-1".to_string(),
                user_id: "user-1".to_string(),
                project_root: None,
                kernel_config: config,
                dynamic_tool_results: HashMap::new(),
                user_action_responses: HashMap::new(),
                sandbox_approval_fingerprints: HashMap::new(),
                command_risk_assessor: None,
            },
            tool_registry: registry,
        }),
    )
    .await
    .expect("streamed tools should execute");

    assert_eq!(result.streamed_tool_outputs.len(), 1);
    assert_eq!(result.streamed_tool_outputs[0].call_id, "call-0");
    assert!(result.streamed_tool_outputs[0].outcome.pending().is_some());
}

#[tokio::test]
async fn test_execute_step_streams_tool_preface_as_commentary() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let file_path = workspace_dir.join("sample.txt");
    std::fs::write(&file_path, "streamed content").expect("sample file should exist");
    let provider = Arc::new(StreamingToolExecutionProvider {
        path: file_path.to_string_lossy().to_string(),
    });
    let (tx, mut rx) = mpsc::channel(16);

    let step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 3,
            call_mode: KernelModelCallMode::WorkingLoop,
            model: "streaming-tool-execution".to_string(),
            max_tokens: None,
            messages: vec![ChatMessage::user("Read the sample file")],
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            reasoning_effort: None,
            speed: Default::default(),
            pre_final_execution_summary: None,
            stream_tx: Some(tx),
            streaming_tool_execution: None,
        },
        provider,
    )
    .await
    .expect("step should succeed");

    assert!(matches!(
        step.disposition,
        GeneralTurnStepDisposition::ContinueWithTools
    ));
    assert!(!step.final_answer_streamed);

    let mut started_phase = None;
    let mut committed_phase = None;
    let mut text_delta = String::new();
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::AssistantTextStarted(data) => {
                started_phase = Some(data.phase);
                assert_eq!(data.round, Some(3));
            }
            KernelStreamEventData::AssistantTextDelta(data) => text_delta.push_str(&data.delta),
            KernelStreamEventData::AssistantTextCommitted(data) => {
                committed_phase = Some(data.phase);
            }
            _ => {}
        }
    }

    assert_eq!(started_phase, Some(AssistantTextPhase::Unknown));
    assert_eq!(committed_phase, Some(AssistantTextPhase::Commentary));
    assert_eq!(text_delta, "Inspecting a file.");
}

#[tokio::test]
async fn test_execute_step_streams_direct_answer_as_final_answer_after_round_done() {
    let provider = Arc::new(DirectStreamingAnswerProvider);
    let (tx, mut rx) = mpsc::channel(8);

    let step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 4,
            call_mode: KernelModelCallMode::WorkingLoop,
            model: "direct-streaming-answer".to_string(),
            max_tokens: None,
            messages: vec![ChatMessage::user("Answer directly")],
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            reasoning_effort: None,
            speed: Default::default(),
            pre_final_execution_summary: None,
            stream_tx: Some(tx),
            streaming_tool_execution: None,
        },
        provider,
    )
    .await
    .expect("step should succeed");

    assert!(matches!(
        step.disposition,
        GeneralTurnStepDisposition::FinalResponse
    ));
    assert!(step.final_answer_streamed);

    let mut started_phase = None;
    let mut committed_phase = None;
    let mut text_delta = String::new();
    let mut completed = false;
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::AssistantTextStarted(data) => {
                started_phase = Some(data.phase);
                assert_eq!(data.round, Some(4));
            }
            KernelStreamEventData::AssistantTextDelta(data) => text_delta.push_str(&data.delta),
            KernelStreamEventData::AssistantTextCompleted(_) => completed = true,
            KernelStreamEventData::AssistantTextCommitted(data) => {
                committed_phase = Some(data.phase);
            }
            _ => {}
        }
    }

    assert_eq!(started_phase, Some(AssistantTextPhase::Unknown));
    assert_eq!(committed_phase, Some(AssistantTextPhase::FinalAnswer));
    assert_eq!(text_delta, "Direct answer.");
    assert!(completed);
}

#[tokio::test]
async fn test_execute_step_maps_provider_explicit_final_phase_to_final_answer() {
    let provider = Arc::new(ExplicitFinalModelBlockProvider);
    let (tx, mut rx) = mpsc::channel(8);

    let step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 5,
            call_mode: KernelModelCallMode::WorkingLoop,
            model: "explicit-final-model-block".to_string(),
            max_tokens: None,
            messages: vec![ChatMessage::user("Answer directly")],
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            reasoning_effort: None,
            speed: Default::default(),
            pre_final_execution_summary: None,
            stream_tx: Some(tx),
            streaming_tool_execution: None,
        },
        provider,
    )
    .await
    .expect("step should succeed");

    assert!(matches!(
        step.disposition,
        GeneralTurnStepDisposition::FinalResponse
    ));
    assert!(step.final_answer_streamed);

    let mut started_phase = None;
    let mut text_delta = String::new();
    let mut final_answer_started = false;
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::AssistantTextStarted(data) => {
                started_phase = Some(data.phase);
                assert_eq!(data.round, Some(5));
                final_answer_started = true;
            }
            KernelStreamEventData::AssistantTextDelta(data) => text_delta.push_str(&data.delta),
            _ => {}
        }
    }

    assert_eq!(started_phase, Some(AssistantTextPhase::FinalAnswer));
    assert_eq!(text_delta, "Explicit final.");
    assert!(final_answer_started);
}

#[tokio::test]
async fn test_run_general_turn_ends_loop_terminal_without_extra_finalization() {
    let provider = Arc::new(DirectTerminalStreamingProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(TestMemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let (tx, mut rx) = mpsc::channel(16);
    let mut ctx = build_ctx_with_stream(Some(tx));

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Answer directly",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("general turn should succeed");

    assert_eq!(result.output_text, "Working answer.");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let mut phases = Vec::new();
    let mut committed_phases = Vec::new();
    let mut deltas = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::AssistantTextStarted(data) => {
                phases.push(data.phase);
            }
            KernelStreamEventData::AssistantTextDelta(data) => deltas.push(data.delta),
            KernelStreamEventData::AssistantTextCommitted(data) => {
                committed_phases.push(data.phase);
            }
            _ => {}
        }
    }

    assert_eq!(phases, vec![AssistantTextPhase::Unknown]);
    assert_eq!(committed_phases, vec![AssistantTextPhase::FinalAnswer]);
    assert_eq!(deltas, vec!["Working answer."]);
}

#[tokio::test]
async fn test_stream_round_executes_tools_during_stream_and_emits_lifecycle_events() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let file_path = workspace_dir.join("sample.txt");
    std::fs::write(&file_path, "streamed content").expect("sample file should exist");

    let provider = Arc::new(StreamingToolExecutionProvider {
        path: file_path.to_string_lossy().to_string(),
    });
    let request = ChatRequest::new_user("Read the sample file", "streaming-tool-execution");
    let registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(registry.as_ref());
    let execution_context = ToolExecutionContext {
        session_id: "agent-a1".to_string(),
        turn_id: "turn-1".to_string(),
        agent_id: "agent-a1".to_string(),
        user_id: "talos".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        dynamic_tool_results: std::collections::HashMap::new(),
        user_action_responses: std::collections::HashMap::new(),
        sandbox_approval_fingerprints: std::collections::HashMap::new(),
        command_risk_assessor: None,
    };
    let (tx, mut rx) = mpsc::channel(16);

    let result = stream_round(
        provider,
        request,
        0,
        KernelModelCallMode::WorkingLoop,
        None,
        Some(&tx),
        Some(StreamingToolExecutionConfig {
            execution_mode: ToolExecutionMode::Parallel,
            max_parallelism: 3,
            execution_context,
            tool_registry: registry,
        }),
    )
    .await
    .expect("stream round should succeed");

    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.streamed_tool_outputs.len(), 1);

    let mut saw_args_delta = false;
    let mut saw_queued = false;
    let mut saw_executing = false;
    let mut saw_result = false;
    let mut saw_complete_semantic = false;
    let mut saw_envelope = false;
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::ToolArgsDelta(_) => saw_args_delta = true,
            KernelStreamEventData::ToolQueued(_) => saw_queued = true,
            KernelStreamEventData::ToolExecuting(_) => saw_executing = true,
            KernelStreamEventData::ToolResult(data) => {
                saw_result = true;
                saw_complete_semantic = data
                    .semantic_events
                    .iter()
                    .any(|item| item == "tool_use_complete");
                saw_envelope = data.envelope.is_some();
            }
            _ => {}
        }
    }

    assert!(saw_args_delta);
    assert!(saw_queued);
    assert!(saw_executing);
    assert!(saw_result);
    assert!(saw_complete_semantic);
    assert!(saw_envelope);
}
