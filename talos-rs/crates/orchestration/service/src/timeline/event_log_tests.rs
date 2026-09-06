use std::sync::Arc;

use talos_agent_api::{AgentMessage, AgentMessageRole, AgentMessageStatus, AgentTurnRef};
use talos_storage::SqliteStorage;
use talos_store::session::DatabaseSessionStore;

use super::TimelineEventService;
use crate::test_support::{commit_test_running_turn, run_test_migrations};
use crate::timeline::TimelineEventId;

async fn fixture() -> (Arc<DatabaseSessionStore>, TimelineEventService) {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .expect("test storage should open"),
    );
    run_test_migrations(storage.pool()).await;
    let store = Arc::new(
        DatabaseSessionStore::new(storage)
            .await
            .expect("Session Store should initialize"),
    );
    let service = TimelineEventService::new(store.clone());
    (store, service)
}

fn user_message(identity: &AgentTurnRef, message_id: &str) -> AgentMessage {
    AgentMessage {
        message_id: message_id.to_string(),
        turn_id: identity.turn_id.clone(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: Vec::new(),
        created_at: 10,
        completed_at: Some(10),
    }
}

#[tokio::test]
async fn timeline_reads_the_canonical_store_without_a_projection_store() {
    let (store, service) = fixture().await;
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let commit = commit_test_running_turn(
        store.as_ref(),
        identity.clone(),
        user_message(&identity, "message-1"),
    )
    .await;

    let events = service
        .list_events_after("session-1", None, 10)
        .await
        .expect("canonical Timeline should load");

    assert_eq!(events.len(), commit.events.len());
    assert_eq!(events[0].event.identity, identity);
    assert_eq!(events[0].event.sequence, 1);
    assert_eq!(events[0].event_id, TimelineEventId::from_seq(1).to_string());
}

#[tokio::test]
async fn timeline_rejects_a_cursor_owned_by_another_session() {
    let (store, service) = fixture().await;
    let first = AgentTurnRef::new("session-1", "turn-1");
    commit_test_running_turn(
        store.as_ref(),
        first.clone(),
        user_message(&first, "message-1"),
    )
    .await;
    let second = AgentTurnRef::new("session-2", "turn-2");
    commit_test_running_turn(
        store.as_ref(),
        second.clone(),
        user_message(&second, "message-2"),
    )
    .await;
    let foreign_cursor = service
        .latest_event_id("session-1")
        .await
        .expect("latest event should load")
        .expect("first Session should have a cursor");

    let error = service
        .list_events_after("session-2", Some(&foreign_cursor), 10)
        .await
        .expect_err("cross-Session cursor must be rejected");

    assert_eq!(error.error_code(), talos_error::ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn only_committed_records_are_published_to_live_subscribers() {
    let (store, service) = fixture().await;
    let mut live = service.subscribe_live_events();
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let commit = commit_test_running_turn(
        store.as_ref(),
        identity.clone(),
        user_message(&identity, "message-1"),
    )
    .await;

    let published = service.publish_committed(commit.events);
    let received = live.recv().await.expect("committed event should publish");

    assert_eq!(received, published[0]);
}
