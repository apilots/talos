use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use std::sync::Arc;
use talos_storage::SqliteStorage;
use talos_types::{
    PendingUserAction, QuestionParams, UserActionParams, UserActionResponse, UserActionState,
};

async fn store() -> DatabaseSessionStore {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    DatabaseSessionStore::new(storage).await.unwrap()
}

fn record(state: UserActionState, version: u64) -> UserActionRecord {
    UserActionRecord {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        action: PendingUserAction {
            action_id: "action-1".to_string(),
            call_id: "call-1".to_string(),
            item_id: None,
            state,
            params: UserActionParams::Question(QuestionParams {
                header: "Choice".to_string(),
                question: "Continue?".to_string(),
                options: Vec::new(),
            }),
        },
        response: None,
        version,
        updated_at: 100,
    }
}

#[tokio::test]
async fn create_is_idempotent_only_for_identical_content() {
    let store = store().await;
    let expected = record(UserActionState::Pending, 0);

    assert_eq!(store.create(expected.clone()).await.unwrap(), expected);
    assert_eq!(store.create(expected.clone()).await.unwrap(), expected);

    let mut conflicting = expected;
    conflicting.action.call_id = "different-call".to_string();
    assert!(matches!(
        store.create(conflicting).await,
        Err(talos_error::TalosError::Conflict(_))
    ));
}

#[tokio::test]
async fn compare_and_set_persists_response_and_increments_version() {
    let store = store().await;
    let current = store
        .create(record(UserActionState::Pending, 0))
        .await
        .unwrap();
    let mut next = current.clone();
    next.action.state = UserActionState::ResponseAccepted;
    next.response = Some(UserActionResponse::SandboxApproval(
        talos_types::SandboxApprovalDecision::Deny,
    ));
    next.version = 1;
    next.updated_at = 101;

    let updated = store.compare_and_set(&current, next.clone()).await.unwrap();
    assert_eq!(updated, next);
    assert_eq!(
        store.get("session-1", "turn-1", "action-1").await.unwrap(),
        Some(next)
    );
}

#[tokio::test]
async fn compare_and_set_rejects_stale_state_or_version() {
    let store = store().await;
    let current = store
        .create(record(UserActionState::Pending, 0))
        .await
        .unwrap();
    let mut next = current.clone();
    next.action.state = UserActionState::Cancelled;
    next.version = 1;

    store.compare_and_set(&current, next.clone()).await.unwrap();

    assert!(matches!(
        store.compare_and_set(&current, next).await,
        Err(talos_error::TalosError::Conflict(_))
    ));
}

#[tokio::test]
async fn records_survive_store_reconstruction() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let first = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let expected = first
        .create(record(UserActionState::ResumeFailed, 3))
        .await
        .unwrap();
    drop(first);

    let reopened = DatabaseSessionStore::new(storage).await.unwrap();
    assert_eq!(
        reopened
            .get("session-1", "turn-1", "action-1")
            .await
            .unwrap(),
        Some(expected)
    );
}
