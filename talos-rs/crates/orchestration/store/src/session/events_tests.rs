use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use crate::session::active_turn::{ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate};
use std::sync::Arc;
use talos_storage::SqliteStorage;
use talos_types::talos::TalosTurnStartedEvent;

async fn store() -> DatabaseSessionStore {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    DatabaseSessionStore::new(storage).await.unwrap()
}

fn started_event(session_id: &str, sequence: u64) -> AgentEvent {
    AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: Some(1000),
        event_type: AgentEventType::TurnStarted,
        turn_id: "turn_1".to_string(),
        message_id: "msg_1".to_string(),
        session_id: session_id.to_string(),
        sequence,
        payload: AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: "req_1".to_string(),
            turn_id: "turn_1".to_string(),
            started_at_ms: 1000,
        }),
    }
}

#[tokio::test]
async fn append_event_assigns_monotonic_event_id() {
    let store = store().await;
    let first = store.append_event(started_event("s1", 0)).await.unwrap();
    let second = store.append_event(started_event("s1", 1)).await.unwrap();

    assert_eq!(first.event_seq, Some(1));
    assert_eq!(first.event_id.as_deref(), Some("ev_00000000000000000001"));
    assert_eq!(second.event_seq, Some(2));
    assert_eq!(second.event_id.as_deref(), Some("ev_00000000000000000002"));
}

#[tokio::test]
async fn list_events_after_uses_event_cursor() {
    let store = store().await;
    let first = store.append_event(started_event("s1", 0)).await.unwrap();
    store.append_event(started_event("s1", 1)).await.unwrap();
    store.append_event(started_event("s2", 0)).await.unwrap();
    let cursor = TimelineEventId::from(first.event_id.unwrap());

    let events = store
        .list_events_after("s1", Some(&cursor), 10)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, 1);
}

#[tokio::test]
async fn oldest_event_id_returns_first_retained_event() {
    let store = store().await;
    let first = store.append_event(started_event("s1", 0)).await.unwrap();
    store.append_event(started_event("s1", 1)).await.unwrap();

    let oldest = store.oldest_event_id("s1").await.unwrap();

    assert_eq!(oldest.map(|id| id.to_string()), first.event_id);
}

#[tokio::test]
async fn gc_does_not_delete_active_turn_events() {
    let store = store().await;
    store
        .append_event(started_event("active", 0))
        .await
        .unwrap();
    store.append_event(started_event("done", 0)).await.unwrap();
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "active".to_string(),
            turn_id: "turn_1".to_string(),
            message_id: Some("msg_1".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            pending_user_action: None,
            updated_at: 1000,
        })
        .await
        .unwrap();

    let deleted = store
        .delete_completed_events_before(2000, 10)
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    let active_events = store.list_events_after("active", None, 10).await.unwrap();
    let done_events = store.list_events_after("done", None, 10).await.unwrap();
    assert_eq!(active_events.len(), 1);
    assert!(done_events.is_empty());
}

#[tokio::test]
async fn gc_keeps_recent_events_per_completed_session() {
    let store = store().await;
    store.append_event(started_event("done", 0)).await.unwrap();
    store.append_event(started_event("done", 1)).await.unwrap();
    store.append_event(started_event("done", 2)).await.unwrap();

    let deleted = store
        .delete_overflow_events_per_session(2, 10)
        .await
        .unwrap();

    assert_eq!(deleted, 1);
    let remaining = store.list_events_after("done", None, 10).await.unwrap();
    assert_eq!(
        remaining
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}
