use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use std::sync::Arc;
use talos_storage::SqliteStorage;

async fn store() -> DatabaseSessionStore {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    DatabaseSessionStore::new(storage).await.unwrap()
}

#[tokio::test]
async fn cancelling_preserves_current_action_id() {
    let store = store().await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "t1".to_string(),
            message_id: Some("m1".to_string()),
            status: ActiveTurnStatus::Cancelling,
            last_event_id: Some(TimelineEventId::from_seq(3)),
            current_action_id: Some("action-1".to_string()),
            updated_at: 10,
        })
        .await
        .unwrap();

    let record = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(record.status, ActiveTurnStatus::Cancelling);
    assert_eq!(record.current_action_id.as_deref(), Some("action-1"));
}

#[tokio::test]
async fn waiting_user_action_persists_only_current_action_id() {
    let store = store().await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "t1".to_string(),
            message_id: Some("m1".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(3)),
            current_action_id: Some("action-1".to_string()),
            updated_at: 10,
        })
        .await
        .unwrap();

    let record = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(record.status, ActiveTurnStatus::WaitingUserAction);
    assert_eq!(record.current_action_id.as_deref(), Some("action-1"));
}
