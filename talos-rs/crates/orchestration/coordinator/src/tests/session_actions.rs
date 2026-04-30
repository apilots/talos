use super::*;

#[tokio::test]
async fn test_submit_tool_approval_routes_to_session_agent() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let approvals = agent.approvals.clone();
    let resumed_inputs = agent.resumed_inputs.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    talos_types::SessionRuntime::submit_tool_approval(
        &coordinator,
        "session-1",
        "call-1",
        "Bash",
        false,
        Some("deny".to_string()),
    )
    .await
    .expect("tool approval should route successfully");

    let approvals = approvals.lock().unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        approvals[0],
        (
            "session-1".to_string(),
            "call-1".to_string(),
            "Bash".to_string(),
            false,
            Some("deny".to_string()),
        )
    );
    drop(approvals);

    let resumed_inputs = resumed_inputs.lock().unwrap();
    assert_eq!(resumed_inputs.len(), 1);
    assert!(resumed_inputs[0].is_empty());
}

#[tokio::test]
async fn test_compact_session_routes_to_session_agent_and_records_usage() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-compact".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 2,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator =
        CoordinatorImpl::new(mock_router, session_store.clone(), TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let compacted_sessions = agent.compacted_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let session = talos_types::SessionRuntime::compact_session(&coordinator, "session-compact")
        .await
        .expect("compact session should succeed");

    assert_eq!(
        compacted_sessions.lock().unwrap().as_slice(),
        &["session-compact".to_string()]
    );
    assert_eq!(session.state.message_count, 2);
    assert_eq!(session.state.usage.total_tokens, 8);
    assert_eq!(session.state.usage.cache_creation_input_tokens, Some(2));
    assert_eq!(session.state.usage.cache_read_input_tokens, Some(4));
}

#[tokio::test]
async fn test_clear_session_routes_to_session_agent_and_resets_session_state() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-clear".to_string(),
        title: "Old Title".to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 6,
        usage: TalosUsage {
            input_tokens: 9,
            output_tokens: 3,
            total_tokens: 12,
            cache_creation_input_tokens: Some(1),
            cache_read_input_tokens: Some(2),
        },
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let cleared_sessions = agent.cleared_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let session = talos_types::SessionRuntime::clear_session(&coordinator, "session-clear")
        .await
        .expect("clear session should succeed");

    assert_eq!(
        cleared_sessions.lock().unwrap().as_slice(),
        &["session-clear".to_string()]
    );
    assert_eq!(session.title, DEFAULT_SESSION_TITLE);
    assert_eq!(session.state.message_count, 0);
    assert_eq!(session.state.usage, TalosUsage::default());
}

#[tokio::test]
async fn test_delete_session_clears_agent_state_and_removes_record() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-delete".to_string(),
        title: "Delete Me".to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 3,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator =
        CoordinatorImpl::new(mock_router, session_store.clone(), TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let cleared_sessions = agent.cleared_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let ack = talos_types::SessionRuntime::delete_session(&coordinator, "session-delete")
        .await
        .expect("delete session should succeed");

    assert!(ack.success);
    assert_eq!(
        cleared_sessions.lock().unwrap().as_slice(),
        &["session-delete".to_string()]
    );
    assert!(coordinator
        .get_session("session-delete")
        .await
        .expect("get session should succeed")
        .is_none());
}

#[tokio::test]
async fn test_delete_session_rejects_active_turn() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-busy".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    coordinator
        .register_agent(
            "test_agent",
            Arc::new(GuardedSessionAgent::new(true, false)),
        )
        .await
        .expect("agent registration should succeed");

    let err = talos_types::SessionRuntime::delete_session(&coordinator, "session-busy")
        .await
        .expect_err("delete should reject active session");
    assert!(matches!(err, TalosError::ValidationError(_)));
    assert!(err.to_string().contains("active"));
}

#[tokio::test]
async fn test_delete_session_rejects_pending_memory_flush() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-flushing".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    coordinator
        .register_agent(
            "test_agent",
            Arc::new(GuardedSessionAgent::new(false, true)),
        )
        .await
        .expect("agent registration should succeed");

    let err = talos_types::SessionRuntime::delete_session(&coordinator, "session-flushing")
        .await
        .expect_err("delete should reject pending memory flush");
    assert!(matches!(err, TalosError::ValidationError(_)));
    assert!(err.to_string().contains("pending"));
}

#[tokio::test]
async fn test_first_turn_assigns_deterministic_session_title() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-1".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator =
        CoordinatorImpl::new(mock_router, session_store.clone(), TalosConfig::default());
    coordinator
        .register_agent("test_agent", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("agent registration should succeed");

    let response = coordinator
        .handle(GatewayRequest {
            session_id: "session-1".to_string(),
            workspace_id: None,
            input: vec![TalosInputItem::Text {
                text: "Fix tui paste handling".to_string(),
            }],
            context: TalosContext {
                source_type: TalosSourceType::Im,
                provider: TalosProvider::Telegram,
                user_id: "user-1".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test-channel".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
        })
        .await
        .expect("request should succeed");

    assert!(response.success);
    let updated = session_store
        .get_session_record("session-1")
        .await
        .expect("load session should succeed")
        .expect("session should exist");
    assert_eq!(updated.title, "Fix tui paste handling");
}

#[tokio::test]
async fn test_cancel_active_turn_routes_to_session_agent_and_is_noop_safe() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-cancel".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let cancelled_sessions = agent.cancelled_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    talos_types::SessionService::cancel_active_turn(&coordinator, "session-cancel")
        .await
        .expect("cancel should route successfully");
    talos_types::SessionService::cancel_active_turn(&coordinator, "session-cancel")
        .await
        .expect("duplicate cancel should still be ok");

    let cancelled_sessions = cancelled_sessions.lock().unwrap();
    assert_eq!(
        cancelled_sessions.as_slice(),
        ["session-cancel".to_string(), "session-cancel".to_string()]
    );
}
