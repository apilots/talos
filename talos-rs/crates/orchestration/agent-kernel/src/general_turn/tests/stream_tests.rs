use super::*;

struct DirectStreamingAnswerProvider;

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

    async fn stream_with_tools(&self, _request: ChatRequest) -> TalosResult<ToolStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(4);
        tokio::spawn(async move {
            tx.send(Ok(ToolStreamEvent::TextDelta {
                delta: "Direct answer.".to_string(),
            }))
            .await
            .expect("text delta should send");
            tx.send(Ok(ToolStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ToolStream::new(rx))
    }

    fn default_model(&self) -> String {
        "direct-streaming-answer".to_string()
    }
}

#[tokio::test]
async fn test_stream_round_normalizes_empty_tool_call_id_before_emitting_start_event() {
    let provider = Arc::new(StreamingToolCallIdProvider);
    let request = ChatRequest::new_user("Search workspace memory", "streaming-tool-call-id");
    let (tx, mut rx) = mpsc::channel(8);

    let result = stream_round(provider, request, 0, Some(&tx), None)
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
            model: "streaming-tool-execution".to_string(),
            max_tokens: None,
            messages: vec![ChatMessage::user("Read the sample file")],
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            native_thinking: None,
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
    let mut text_delta = String::new();
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::AssistantTextStarted(data) => {
                started_phase = Some(data.phase);
                assert_eq!(data.round, Some(3));
            }
            KernelStreamEventData::AssistantTextDelta(data) => text_delta.push_str(&data.delta),
            _ => {}
        }
    }

    assert_eq!(started_phase, Some(KernelAssistantTextPhase::Commentary));
    assert_eq!(text_delta, "Inspecting a file.");
}

#[tokio::test]
async fn test_execute_step_streams_direct_answer_as_final_answer() {
    let provider = Arc::new(DirectStreamingAnswerProvider);
    let (tx, mut rx) = mpsc::channel(8);

    let step = execute_general_turn_step(
        GeneralTurnStepRequest {
            round: 4,
            model: "direct-streaming-answer".to_string(),
            max_tokens: None,
            messages: vec![ChatMessage::user("Answer directly")],
            tool_definitions: Vec::new(),
            parallel_tool_calls: false,
            native_thinking: None,
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
    let mut completed = false;
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::AssistantTextStarted(data) => {
                started_phase = Some(data.phase);
                assert_eq!(data.round, Some(4));
            }
            KernelStreamEventData::AssistantTextDelta(data) => text_delta.push_str(&data.delta),
            KernelStreamEventData::AssistantTextCompleted(_) => completed = true,
            _ => {}
        }
    }

    assert_eq!(started_phase, Some(KernelAssistantTextPhase::FinalAnswer));
    assert_eq!(text_delta, "Direct answer.");
    assert!(completed);
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
    let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
    let execution_context = ToolExecutionContext {
        session_id: "agent-a1".to_string(),
        user_id: "talos".to_string(),
        project_root: None,
        kernel_config: Arc::new(AgentKernelConfig::default()),
        memory_store,
        pre_approved_call_ids: std::collections::HashSet::new(),
        dynamic_tool_results: std::collections::HashMap::new(),
    };
    let (tx, mut rx) = mpsc::channel(16);

    let result = stream_round(
        provider,
        request,
        0,
        Some(&tx),
        Some(StreamingToolExecutionConfig {
            execution_mode: ToolExecutionMode::Parallel,
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
