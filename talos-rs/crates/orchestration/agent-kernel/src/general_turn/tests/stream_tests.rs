use super::*;

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
    while let Ok(event) = rx.try_recv() {
        match event.data {
            KernelStreamEventData::ToolArgsDelta(_) => saw_args_delta = true,
            KernelStreamEventData::ToolQueued(_) => saw_queued = true,
            KernelStreamEventData::ToolExecuting(_) => saw_executing = true,
            KernelStreamEventData::ToolResult(_) => saw_result = true,
            _ => {}
        }
    }

    assert!(saw_args_delta);
    assert!(saw_queued);
    assert!(saw_executing);
    assert!(saw_result);
}
