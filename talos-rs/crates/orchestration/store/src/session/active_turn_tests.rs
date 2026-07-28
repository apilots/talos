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
async fn cancelling_clears_pending_user_action() {
    let store = store().await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "t1".to_string(),
            message_id: Some("m1".to_string()),
            status: ActiveTurnStatus::Cancelling,
            last_event_id: Some(TimelineEventId::from_seq(3)),
            pending_user_action: Some(PendingUserAction::DynamicToolResult(
                PendingDynamicToolResult {
                    item_id: Some("tool-1".to_string()),
                    call_id: "c1".to_string(),
                    tool_name: "tool".to_string(),
                },
            )),
            updated_at: 10,
        })
        .await
        .unwrap();

    let record = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(record.status, ActiveTurnStatus::Cancelling);
    assert_eq!(record.pending_user_action, None);
}
