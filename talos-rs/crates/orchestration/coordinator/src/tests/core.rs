use super::*;
use talos_types::gateway::{GatewayResumeContext, GatewayResumeToolCall};

#[tokio::test]
async fn test_coordinator_creation() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

    assert_eq!(coordinator.agents.read().await.len(), 0);
}

#[tokio::test]
async fn test_register_agent() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

    let mock_agent = Arc::new(talos_agent::MockAgent::new());
    let result = coordinator.register_agent("test_agent", mock_agent).await;

    assert!(result.is_ok());
    assert_eq!(coordinator.agents.read().await.len(), 1);
}

#[tokio::test]
async fn test_drain_memory_flushes_delegates_to_registered_agents() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

    let drained = Arc::new(AtomicUsize::new(0));
    coordinator
        .register_agent(
            "agent-a",
            Arc::new(TrackingFlushAgent::new(1, drained.clone())),
        )
        .await
        .expect("first agent should register");
    coordinator
        .register_agent(
            "agent-b",
            Arc::new(TrackingFlushAgent::new(2, drained.clone())),
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
async fn test_handle_stream_returns_orchestrator_events() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
    coordinator
        .register_agent("test_agent", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("agent registration should succeed");

    let mut stream = coordinator
        .handle_stream(GatewayRequest {
            session_id: "test-session".to_string(),
            turn_id: None,
            workspace_id: None,
            resume_context: None,
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
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
        .expect("stream request should succeed");

    let mut event_types = Vec::new();
    while let Some(event) = stream.recv().await {
        event_types.push(event.event_type.clone());
        if event.event_type == talos_types::talos::TalosTimelineEventType::TurnCompleted {
            break;
        }
    }

    assert!(event_types.contains(&talos_types::talos::TalosTimelineEventType::TurnStarted));
    assert!(event_types.contains(&talos_types::talos::TalosTimelineEventType::ItemStarted));
    assert!(event_types.contains(&talos_types::talos::TalosTimelineEventType::ItemDelta));
    assert!(event_types.contains(&talos_types::talos::TalosTimelineEventType::ItemCompleted));
    assert!(event_types.contains(&talos_types::talos::TalosTimelineEventType::TurnCompleted));
}

#[tokio::test]
async fn test_handle_stream_preserves_resume_turn_and_message() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let stream_requests = agent.stream_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let mut stream = coordinator
        .handle_stream(GatewayRequest {
            session_id: "test-session".to_string(),
            turn_id: Some("turn-original".to_string()),
            workspace_id: None,
            resume_context: Some(GatewayResumeContext {
                tool_call: Some(GatewayResumeToolCall {
                    item_id: "tool-original".to_string(),
                    call_id: "call-1".to_string(),
                    tool_name: "Bash".to_string(),
                    arguments: serde_json::json!({"command":"pwd"}).into(),
                }),
            }),
            input: Vec::new(),
            context: TalosContext {
                source_type: TalosSourceType::Im,
                provider: TalosProvider::Telegram,
                user_id: "user-1".to_string(),
                message_id: "message-original".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test-channel".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
        })
        .await
        .expect("stream request should succeed");

    while stream.recv().await.is_some() {}

    let stream_requests = stream_requests.lock().unwrap();
    let request = stream_requests
        .first()
        .expect("resume stream request should reach agent");
    assert_eq!(request.turn_id.as_deref(), Some("turn-original"));
    assert_eq!(request.session.context.message_id, "message-original");
    assert!(request.input.is_empty());
    assert!(request.resume_context.is_some());
}
