use talos_storage::SqliteStorage;
use talos_types::{ModelRef, StopReason, TokenUsage};

use crate::migration::{run_migrations, test_migrations_dir};

use super::{DatabaseProfileStore, ProfileStore, TurnUsageUpdate};

async fn store() -> DatabaseProfileStore {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    DatabaseProfileStore::new(pool)
}

fn usage(input_tokens: u32, output_tokens: u32) -> TokenUsage {
    usage_with_cache(input_tokens, output_tokens, 1, 2)
}

fn usage_with_cache(
    input_tokens: u32,
    output_tokens: u32,
    cache_creation_input_tokens: u32,
    cache_read_input_tokens: u32,
) -> TokenUsage {
    TokenUsage {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
        cache_creation_input_tokens: Some(cache_creation_input_tokens),
        cache_read_input_tokens: Some(cache_read_input_tokens),
    }
}

fn update(request_id: &str, stop_reason: StopReason, usage: TokenUsage) -> TurnUsageUpdate {
    TurnUsageUpdate {
        user_id: "user-1".to_string(),
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        request_id: request_id.to_string(),
        model_ref: ModelRef {
            provider: "anthropic".to_string(),
            id: "claude".to_string(),
        },
        usage,
        usage_available: true,
        stop_reason,
        timezone: "UTC".to_string(),
        completed_at_ms: 1_000,
        usage_date: "2026-07-16".to_string(),
    }
}

#[tokio::test]
async fn waiting_usage_is_hidden_until_the_turn_finalizes() {
    let store = store().await;

    store
        .record_turn_usage(update(
            "request-1",
            StopReason::WaitingUserAction,
            usage(10, 2),
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .usage_summary("user-1")
            .await
            .unwrap()
            .usage
            .total_tokens,
        0
    );

    store
        .record_turn_usage(update("request-2", StopReason::EndTurn, usage(20, 3)))
        .await
        .unwrap();

    let summary = store.usage_summary("user-1").await.unwrap();
    assert_eq!(summary.usage, usage_with_cache(30, 5, 2, 4));
    assert!(summary.usage_available);
    assert_eq!(summary.usage_session_count, 1);
    assert_eq!(summary.usage_started_at_ms, Some(1_000));

    let daily = store
        .daily_usage("user-1", "2026-07-16", "2026-07-16")
        .await
        .unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].usage, usage_with_cache(30, 5, 2, 4));
}

#[tokio::test]
async fn duplicate_execution_segment_is_not_counted_twice() {
    let store = store().await;
    let waiting = update("request-1", StopReason::WaitingUserAction, usage(10, 2));

    store.record_turn_usage(waiting.clone()).await.unwrap();
    store.record_turn_usage(waiting).await.unwrap();
    store
        .record_turn_usage(update("request-2", StopReason::EndTurn, usage(20, 3)))
        .await
        .unwrap();

    assert_eq!(
        store.usage_summary("user-1").await.unwrap().usage,
        usage_with_cache(30, 5, 2, 4)
    );
}

#[tokio::test]
async fn usage_summary_counts_distinct_finalized_sessions() {
    let store = store().await;
    store
        .record_turn_usage(update("request-1", StopReason::EndTurn, usage(10, 2)))
        .await
        .unwrap();
    let mut second_session = update("request-2", StopReason::EndTurn, usage(20, 3));
    second_session.session_id = "session-2".to_string();
    second_session.turn_id = "turn-2".to_string();
    store.record_turn_usage(second_session).await.unwrap();

    assert_eq!(
        store
            .usage_summary("user-1")
            .await
            .unwrap()
            .usage_session_count,
        2
    );
}

#[tokio::test]
async fn cancellation_does_not_delete_finalized_usage() {
    let store = store().await;
    store
        .record_turn_usage(update("request-1", StopReason::EndTurn, usage(10, 2)))
        .await
        .unwrap();
    store
        .record_turn_usage(update(
            "request-2",
            StopReason::Cancelled,
            TokenUsage::default(),
        ))
        .await
        .unwrap();

    assert_eq!(
        store.usage_summary("user-1").await.unwrap().usage,
        usage(10, 2)
    );
}

#[tokio::test]
async fn aggregate_usage_is_scoped_to_the_requesting_user() {
    let store = store().await;
    store
        .record_turn_usage(update("request-1", StopReason::EndTurn, usage(10, 2)))
        .await
        .unwrap();
    let mut other = update("request-2", StopReason::EndTurn, usage(500, 100));
    other.user_id = "user-2".to_string();
    other.session_id = "session-2".to_string();
    other.turn_id = "turn-2".to_string();
    store.record_turn_usage(other).await.unwrap();

    assert_eq!(
        store.usage_summary("user-1").await.unwrap().usage,
        usage(10, 2)
    );
}

#[tokio::test]
async fn deletion_keeps_recent_and_unfinished_usage() {
    let store = store().await;
    let mut oldest = update("request-1", StopReason::EndTurn, usage(10, 2));
    oldest.completed_at_ms = 100;
    store.record_turn_usage(oldest).await.unwrap();

    let mut another_old = update("request-2", StopReason::EndTurn, usage(20, 3));
    another_old.session_id = "session-2".to_string();
    another_old.turn_id = "turn-2".to_string();
    another_old.completed_at_ms = 150;
    store.record_turn_usage(another_old).await.unwrap();

    let mut recent = update("request-3", StopReason::EndTurn, usage(30, 4));
    recent.session_id = "session-3".to_string();
    recent.turn_id = "turn-3".to_string();
    recent.completed_at_ms = 300;
    store.record_turn_usage(recent).await.unwrap();

    let mut unfinished = update("request-4", StopReason::WaitingUserAction, usage(40, 5));
    unfinished.session_id = "session-4".to_string();
    unfinished.turn_id = "turn-4".to_string();
    unfinished.completed_at_ms = 50;
    store.record_turn_usage(unfinished).await.unwrap();

    assert_eq!(store.delete_finalized_before(200, 1).await.unwrap(), 1);
    assert_eq!(store.delete_finalized_before(200, 10).await.unwrap(), 1);

    let finalized: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profile_turn_usage WHERE finalized = 1")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    let unfinished: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM profile_turn_usage WHERE finalized = 0")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(finalized, 1);
    assert_eq!(unfinished, 1);
}
