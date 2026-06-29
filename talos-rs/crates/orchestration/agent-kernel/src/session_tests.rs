use super::*;
use std::sync::Arc;
use talos_memory::MemoryStore;
use uuid::Uuid;

#[tokio::test]
async fn test_load_session_replays_full_branch_history_without_invocation_filter() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir));
    let service = MemoryBackedKernelTranscriptService::new(memory_store);
    let mut session = InvocationSession {
        session_id: "session-1".to_string(),
        ..InvocationSession::default()
    };

    service
        .append_event(
            &mut session,
            KernelTranscriptEvent::new(
                "session-1",
                "invocation-1",
                ExecutionBranch::ReactiveLoop,
                EventAuthor::User,
                KernelTranscriptEventKind::UserMessage,
                KernelTranscriptPayload::Message {
                    message: ChatMessage::user("hello"),
                },
            ),
        )
        .await
        .expect("first append should succeed");
    service
        .append_event(
            &mut session,
            KernelTranscriptEvent::new(
                "session-1",
                "invocation-2",
                ExecutionBranch::ReactiveLoop,
                EventAuthor::Agent,
                KernelTranscriptEventKind::AgentMessage,
                KernelTranscriptPayload::Message {
                    message: ChatMessage::assistant("hi again"),
                },
            ),
        )
        .await
        .expect("second append should succeed");

    let loaded = service
        .load_session("session-1", None, Some(ExecutionBranch::ReactiveLoop))
        .await
        .expect("load should succeed");

    assert_eq!(loaded.events.len(), 2);
    assert_eq!(loaded.events[0].invocation_id, "invocation-1");
    assert_eq!(loaded.events[1].invocation_id, "invocation-2");
}

#[tokio::test]
async fn test_load_session_replays_state_delta_into_session_state() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir));
    let service = MemoryBackedKernelTranscriptService::new(memory_store);
    let mut session = InvocationSession {
        session_id: "session-1".to_string(),
        ..InvocationSession::default()
    };

    let mut event = KernelTranscriptEvent::new(
        "session-1",
        "invocation-1",
        ExecutionBranch::ReactiveLoop,
        EventAuthor::Agent,
        KernelTranscriptEventKind::Planning,
        KernelTranscriptPayload::StateOnly,
    );
    event.actions.state_delta.push(crate::StateDelta {
        namespace: StateNamespace::Session,
        key: "plan_seen".to_string(),
        value: serde_json::json!(true),
    });

    service
        .append_event(&mut session, event)
        .await
        .expect("append should succeed");

    let loaded = service
        .load_session("session-1", None, Some(ExecutionBranch::ReactiveLoop))
        .await
        .expect("load should succeed");

    assert_eq!(
        loaded.state.get(StateNamespace::Session, "plan_seen"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn test_reset_session_clears_replayed_session_state_deltas() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory_store = Arc::new(MemoryStore::new(workspace_dir));
    let service = MemoryBackedKernelTranscriptService::new(memory_store.clone());
    let mut session = InvocationSession {
        session_id: "session-1".to_string(),
        ..InvocationSession::default()
    };

    for (key, value) in [
        ("session_compaction_state", serde_json::json!("blocked")),
        (
            "session_continuity",
            serde_json::json!({"recent_goal": "resume"}),
        ),
    ] {
        let mut event = KernelTranscriptEvent::new(
            "session-1",
            "invocation-1",
            ExecutionBranch::ReactiveLoop,
            EventAuthor::Agent,
            KernelTranscriptEventKind::StateUpdate,
            KernelTranscriptPayload::StateOnly,
        );
        event.actions.state_delta.push(crate::StateDelta {
            namespace: StateNamespace::Session,
            key: key.to_string(),
            value,
        });

        service
            .append_event(&mut session, event)
            .await
            .expect("append should succeed");
    }

    let loaded_before_reset = service
        .load_session("session-1", None, Some(ExecutionBranch::ReactiveLoop))
        .await
        .expect("load before reset should succeed");
    assert!(loaded_before_reset
        .state
        .get(StateNamespace::Session, "session_compaction_state")
        .is_some());
    assert!(loaded_before_reset
        .state
        .get(StateNamespace::Session, "session_continuity")
        .is_some());

    memory_store
        .reset_session("session-1")
        .await
        .expect("reset session should succeed");

    let loaded_after_reset = service
        .load_session("session-1", None, Some(ExecutionBranch::ReactiveLoop))
        .await
        .expect("load after reset should succeed");
    assert!(loaded_after_reset.events.is_empty());
    assert!(loaded_after_reset
        .state
        .bag(StateNamespace::Session)
        .is_none());
}
