use super::*;
use talos_types::agent_dispatch::{AgentDispatchResumeContext, AgentDispatchResumeToolCall};

#[tokio::test]
async fn test_coordinator_creation() {
    let mock_router = Arc::new(MockRouter::new());
    let coordinator = CoordinatorImpl::new(mock_router);

    assert_eq!(coordinator.registry().len().await, 0);
}

#[tokio::test]
async fn test_register_agent() {
    let mock_router = Arc::new(MockRouter::new());
    let coordinator = CoordinatorImpl::new(mock_router);

    let mock_agent = Arc::new(talos_agent::MockAgent::new());
    let result = coordinator.register_agent("mock", mock_agent).await;

    assert!(result.is_ok());
    assert_eq!(coordinator.registry().len().await, 1);
}

#[tokio::test]
async fn test_drain_memory_flushes_delegates_to_registered_agents() {
    let mock_router = Arc::new(MockRouter::new());
    let coordinator = CoordinatorImpl::new(mock_router);

    let drained = Arc::new(AtomicUsize::new(0));
    coordinator
        .register_agent(
            "agent-a",
            Arc::new(TrackingFlushAgent::new("agent-a", 1, drained.clone())),
        )
        .await
        .expect("first agent should register");
    coordinator
        .register_agent(
            "agent-b",
            Arc::new(TrackingFlushAgent::new("agent-b", 2, drained.clone())),
        )
        .await
        .expect("second agent should register");

    assert_eq!(coordinator.pending_memory_flushes().await, 3);
    coordinator
        .drain_memory_flushes(Duration::from_secs(1))
        .await
        .expect("coordinator should drain all agents");
    assert_eq!(drained.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_handle_stream_returns_agent_events() {
    let mock_router = Arc::new(MockRouter::with_target(Some("mock")));
    let coordinator = CoordinatorImpl::new(mock_router);
    coordinator
        .register_agent("mock", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("agent registration should succeed");

    let context = TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("test-channel".to_string()),
        thread_id: None,
        project_root: None,
        environment: None,
    };
    let mut stream = coordinator
        .handle_stream(AgentDispatchParams {
            turn_id: "turn-stream".to_string(),
            session: test_session("test-session", context.clone()),
            resume_context: None,
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        })
        .await
        .expect("stream request should succeed");

    let mut event_types = Vec::new();
    while let Some(event) = stream.recv().await {
        event_types.push(event.event_type.clone());
        if event.event_type == talos_types::talos::AgentEventType::TurnCompleted {
            break;
        }
    }

    assert!(event_types.contains(&talos_types::talos::AgentEventType::TurnStarted));
    assert!(event_types.contains(&talos_types::talos::AgentEventType::ItemStarted));
    assert!(event_types.contains(&talos_types::talos::AgentEventType::ItemDelta));
    assert!(event_types.contains(&talos_types::talos::AgentEventType::ItemCompleted));
    assert!(event_types.contains(&talos_types::talos::AgentEventType::TurnCompleted));
}

#[tokio::test]
async fn test_handle_request_uses_prepared_session() {
    let mock_router = Arc::new(MockRouter::new());
    let context = TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("test-channel".to_string()),
        thread_id: None,
        project_root: None,
        environment: None,
    };
    let coordinator = CoordinatorImpl::new(mock_router);
    coordinator
        .register_agent("test_agent", Arc::new(RecordingStreamAgent::new()))
        .await
        .expect("agent registration should succeed");

    let response = coordinator
        .handle(AgentDispatchParams {
            turn_id: "turn-title".to_string(),
            session: test_session("test-session", context.clone()),
            resume_context: None,
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        })
        .await
        .expect("request should succeed");

    assert_eq!(response.output_text, "ok");
    assert_eq!(response.session_id, "test-session");
}

#[tokio::test]
async fn test_handle_request_uses_resume_api_for_resume_context() {
    let mock_router = Arc::new(MockRouter::new());
    let context = TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-original".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("test-channel".to_string()),
        thread_id: None,
        project_root: None,
        environment: None,
    };
    let coordinator = CoordinatorImpl::new(mock_router);
    let agent = Arc::new(RecordingStreamAgent::new());
    let run_requests = agent.run_requests.clone();
    let resume_requests = agent.resume_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let response = coordinator
        .handle(AgentDispatchParams {
            turn_id: "turn-original".to_string(),
            session: test_session("test-session", context.clone()),
            resume_context: Some(AgentDispatchResumeContext {
                tool_call: Some(AgentDispatchResumeToolCall {
                    item_id: "tool-original".to_string(),
                    call_id: "call-1".to_string(),
                    tool_name: "Bash".to_string(),
                }),
            }),
            input: Vec::new(),
        })
        .await
        .expect("resume request should succeed");

    assert_eq!(response.output_text, "ok");
    assert!(run_requests.lock().unwrap().is_empty());
    let resume_requests = resume_requests.lock().unwrap();
    let request = resume_requests
        .first()
        .expect("resume request should reach agent");
    assert_eq!(request.run.turn_id(), "turn-original");
    assert_eq!(request.run.context.message_id, "message-original");
    assert!(request.run.resume_context.is_some());
}

#[tokio::test]
async fn test_handle_request_uses_routed_agent_from_registry() {
    let mock_router = Arc::new(MockRouter::with_target(Some("reviewer")));
    let context = TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("test-channel".to_string()),
        thread_id: None,
        project_root: None,
        environment: None,
    };
    let coordinator = CoordinatorImpl::new(mock_router);
    let default_agent = Arc::new(RecordingStreamAgent::with_id("default"));
    let reviewer_agent = Arc::new(RecordingStreamAgent::with_id("reviewer"));
    let default_requests = default_agent.run_requests.clone();
    let reviewer_requests = reviewer_agent.run_requests.clone();

    coordinator
        .register_agent("default", default_agent)
        .await
        .expect("default agent registration should succeed");
    coordinator
        .register_agent("reviewer", reviewer_agent)
        .await
        .expect("reviewer agent registration should succeed");

    let response = coordinator
        .handle(AgentDispatchParams {
            turn_id: "turn-routed".to_string(),
            session: test_session("test-session", context.clone()),
            resume_context: None,
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        })
        .await
        .expect("request should succeed");

    assert_eq!(response.output_text, "ok");
    assert!(default_requests.lock().unwrap().is_empty());
    let reviewer_requests = reviewer_requests.lock().unwrap();
    let request = reviewer_requests
        .first()
        .expect("routed agent should receive request");
    assert_eq!(request.turn_id(), "turn-routed");
    assert_eq!(request.context.turn_id, "turn-routed");
}

#[tokio::test]
async fn test_handle_stream_preserves_resume_turn_and_message() {
    let mock_router = Arc::new(MockRouter::new());
    let coordinator = CoordinatorImpl::new(mock_router);
    let agent = Arc::new(RecordingStreamAgent::new());
    let stream_requests = agent.stream_requests.clone();
    let resume_stream_requests = agent.resume_stream_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let context = TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-original".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("test-channel".to_string()),
        thread_id: None,
        project_root: None,
        environment: None,
    };
    let mut session = test_session("test-session", context);
    session.context.workspace_id = Some("workspace-alpha".to_string());
    let mut stream = coordinator
        .handle_stream(AgentDispatchParams {
            turn_id: "turn-original".to_string(),
            session,
            resume_context: Some(AgentDispatchResumeContext {
                tool_call: Some(AgentDispatchResumeToolCall {
                    item_id: "tool-original".to_string(),
                    call_id: "call-1".to_string(),
                    tool_name: "Bash".to_string(),
                }),
            }),
            input: Vec::new(),
        })
        .await
        .expect("stream request should succeed");

    while stream.recv().await.is_some() {}

    assert!(stream_requests.lock().unwrap().is_empty());
    let resume_stream_requests = resume_stream_requests.lock().unwrap();
    let request = resume_stream_requests
        .first()
        .expect("resume stream request should reach agent");
    assert_eq!(request.run.turn_id(), "turn-original");
    assert_eq!(request.run.context.turn_id, "turn-original");
    assert_eq!(request.run.context.message_id, "message-original");
    assert_eq!(
        request.run.context.workspace_id.as_deref(),
        Some("workspace-alpha")
    );
    assert!(request.run.input.is_empty());
    assert!(request.run.resume_context.is_some());
}
