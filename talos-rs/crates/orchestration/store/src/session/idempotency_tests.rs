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
async fn reserve_uses_existing_record_on_conflict() {
    let store = store().await;
    let first = store
        .reserve("s1", "key", "hash", "turn_1", 100, Some(1000))
        .await
        .unwrap();
    let second = store
        .reserve("s1", "key", "hash", "turn_2", 101, Some(1000))
        .await
        .unwrap();

    assert!(matches!(first, IdempotencyReservation::Reserved(_)));
    let IdempotencyReservation::Existing(record) = second else {
        panic!("second reservation should return existing row");
    };
    assert_eq!(record.turn_id, "turn_1");
}

#[tokio::test]
async fn stale_pending_records_mark_failed() {
    let store = store().await;
    store
        .reserve("s1", "key", "hash", "turn_1", 100, Some(1000))
        .await
        .unwrap();

    let affected = store.mark_stale_pending_failed(200, 10).await.unwrap();
    assert_eq!(affected, 1);
    let IdempotencyReservation::Existing(record) = store
        .reserve("s1", "key", "hash", "turn_2", 300, Some(1000))
        .await
        .unwrap()
    else {
        panic!("expected existing record");
    };
    assert_eq!(record.status, IdempotencyStatus::FailedNoFirstEvent);
}

#[tokio::test]
async fn reserve_replaces_expired_record() {
    let store = store().await;
    store
        .reserve("s1", "key", "hash", "turn_1", 100, Some(200))
        .await
        .unwrap();

    let reservation = store
        .reserve("s1", "key", "hash", "turn_2", 300, Some(500))
        .await
        .unwrap();

    let IdempotencyReservation::Reserved(record) = reservation else {
        panic!("expired key should allow a new reservation");
    };
    assert_eq!(record.turn_id, "turn_2");
}
