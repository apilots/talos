use super::*;

#[tokio::test]
async fn test_get_session_turns_returns_ordered_items() {
    let session_id = format!("history-{}", Uuid::new_v4());
    let store = MemoryStore::new_default();
    let expected_items = vec![
        TalosOutputItem::UserInput {
            text: "user question".to_string(),
        },
        TalosOutputItem::Reasoning {
            text: "think first".to_string(),
        },
        TalosOutputItem::ToolCall {
            call_id: "call-1".to_string(),
            name: "Read".to_string(),
            arguments: serde_json::json!({"path":"/tmp/demo.txt"}).into(),
        },
        TalosOutputItem::ToolResult {
            call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            content: vec![TalosContentItem::Text {
                text: "demo".to_string(),
            }],
            is_error: false,
        },
        TalosOutputItem::OutputText {
            text: "final answer".to_string(),
        },
    ];
    let turn: TalosTurn = serde_json::from_value(serde_json::json!({
        "turn_id": format!("turn-{}", Uuid::new_v4()),
        "session_id": session_id,
        "seq": 1,
        "timestamp": test_timestamp(),
        "items": expected_items.clone(),
        "usage": {
            "input_tokens": 10,
            "output_tokens": 12,
            "total_tokens": 22
        },
        "stop_reason": "end_turn"
    }))
    .expect("turn should deserialize");
    store
        .append_turn(&turn)
        .await
        .expect("append turn should succeed");

    let coordinator = CoordinatorImpl::new(
        Arc::new(MockRouter::new()),
        Arc::new(talos_session_store::MockSessionStore::new()),
        TalosConfig::default(),
    );

    let turns = coordinator
        .get_session_turns(&session_id)
        .await
        .expect("session turns should load");

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].output.len(), expected_items.len());
    assert!(matches!(
        turns[0].output.first(),
        Some(TalosOutputItem::UserInput { text }) if text == "user question"
    ));
    assert!(matches!(
        turns[0].output.get(1),
        Some(TalosOutputItem::Reasoning { text }) if text == "think first"
    ));
    assert!(matches!(
        turns[0].output.get(2),
        Some(TalosOutputItem::ToolCall { name, .. }) if name == "Read"
    ));
    assert!(matches!(
        turns[0].output.get(3),
        Some(TalosOutputItem::ToolResult { tool_name, .. }) if tool_name == "Read"
    ));
    assert!(matches!(
        turns[0].output.get(4),
        Some(TalosOutputItem::OutputText { text }) if text == "final answer"
    ));
    assert_eq!(turns[0].usage.total_tokens, 22);
    assert_eq!(turns[0].stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn test_remote_session_defaults_project_root_to_workspace_dir() {
    let workspace_dir = std::env::temp_dir().join(format!("talos-coordinator-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");

    let mut config = TalosConfig::default();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mock_router = Arc::new(MockRouter::new());
    let session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, session_store, config);
    coordinator
        .register_agent("default", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("default agent should register");

    let session = talos_types::SessionService::create_session(
        &coordinator,
        TalosContext::new(
            TalosSourceType::Cli,
            TalosProvider::Cli,
            "user-1",
            "message-1",
            "trace-1",
        )
        .with_environment(TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/client/path"),
            connection_mode: TalosConnectionMode::Remote,
        }),
        None,
        None,
        None,
        None,
    )
    .await
    .expect("session creation should succeed");

    assert_eq!(
        session.context.project_root.as_deref(),
        Some(workspace_dir.as_path())
    );
}

#[tokio::test]
async fn test_remote_session_ignores_current_directory_patch_for_root_resolution() {
    let workspace_dir = std::env::temp_dir().join(format!("talos-coordinator-{}", Uuid::new_v4()));
    let custom_dir = std::env::temp_dir().join(format!("talos-project-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
    std::fs::create_dir_all(&custom_dir).expect("custom dir should be created");

    let mut config = TalosConfig::default();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mock_router = Arc::new(MockRouter::new());
    let session_store = Arc::new(TitleTestSessionStore::new(SessionRecord {
        id: "session-1".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext::new(
            TalosSourceType::Cli,
            TalosProvider::Cli,
            "user-1",
            "message-1",
            "trace-1",
        )
        .with_environment(TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/client/path"),
            connection_mode: TalosConnectionMode::Remote,
        }),
        model: config.llm.model.clone(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: Some(custom_dir.clone()),
    }));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, config);

    talos_types::SessionService::patch_session(
        &coordinator,
        "session-1",
        SessionPatch {
            conf: None,
            project_id: None,
            current_directory: Some(OptionalCurrentDirectoryPatch::Set {
                path: custom_dir.clone(),
            }),
        },
    )
    .await
    .expect("patch should succeed");

    let session = talos_types::SessionService::get_session(&coordinator, "session-1")
        .await
        .expect("get session should succeed")
        .expect("session should exist");

    assert_eq!(
        session.context.project_root.as_deref(),
        Some(workspace_dir.as_path())
    );
}

#[tokio::test]
async fn test_get_session_turns_includes_tool_results() {
    let session_id = format!("history-tool-{}", Uuid::new_v4());
    let store = MemoryStore::new_default();
    let turn: TalosTurn = serde_json::from_value(serde_json::json!({
        "turn_id": format!("turn-{}", Uuid::new_v4()),
        "session_id": session_id,
        "seq": 1,
        "timestamp": test_timestamp(),
        "items": [
            {
                "type": "tool_call",
                "call_id": "call-tool",
                "name": "Bash",
                "arguments": {"value": {"command": "pwd"}}
            },
            {
                "type": "tool_result",
                "call_id": "call-tool",
                "tool_name": "Bash",
                "content": [
                    {
                        "type": "json",
                        "value": {"value": {"output": "/tmp\n", "exit_code": 0}}
                    }
                ],
                "is_error": false
            }
        ],
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        },
        "stop_reason": "end_turn"
    }))
    .expect("turn should deserialize");
    store
        .append_turn(&turn)
        .await
        .expect("append turn should succeed");

    let coordinator = CoordinatorImpl::new(
        Arc::new(MockRouter::new()),
        Arc::new(talos_session_store::MockSessionStore::new()),
        TalosConfig::default(),
    );

    let turns = coordinator
        .get_session_turns(&session_id)
        .await
        .expect("session turns should load");

    assert!(matches!(
        turns[0].output.get(1),
        Some(TalosOutputItem::ToolResult { tool_name, .. }) if tool_name == "Bash"
    ));
}
