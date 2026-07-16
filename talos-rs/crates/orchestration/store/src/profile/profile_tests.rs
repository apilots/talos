use sqlx::SqlitePool;
use talos_types::{ModelRef, StopReason, TokenUsage};

use crate::migration::run_migrations;

use super::{DatabaseProfileStore, ProfileStore, TurnUsageUpdate};

async fn store() -> DatabaseProfileStore {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();
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
            StopReason::WaitingDynamicInput,
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
    let waiting = update("request-1", StopReason::WaitingDynamicInput, usage(10, 2));

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
