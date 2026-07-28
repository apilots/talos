use std::sync::Arc;

use crate::test_support::run_test_migrations;
use chrono::{DateTime, Utc};
use talos_config::ProfileUsageRetentionConfig;
use talos_storage::SqliteStorage;
use talos_store::profile::{DatabaseProfileStore, ProfileStore, TurnUsageUpdate};
use talos_types::{ModelRef, StopReason, TokenUsage};

use super::ProfileUsageRetention;

fn update(id: &str, completed_at_ms: i64) -> TurnUsageUpdate {
    TurnUsageUpdate {
        user_id: "user-1".to_string(),
        session_id: format!("session-{id}"),
        turn_id: format!("turn-{id}"),
        request_id: format!("request-{id}"),
        model_ref: ModelRef {
            provider: "anthropic".to_string(),
            id: "claude".to_string(),
        },
        usage: TokenUsage {
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        usage_available: true,
        stop_reason: StopReason::EndTurn,
        timezone: "UTC".to_string(),
        completed_at_ms,
        usage_date: "2026-07-16".to_string(),
    }
}

#[tokio::test]
async fn retention_stops_after_configured_batch_limit() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let pool = storage.pool().clone();
    run_test_migrations(&pool).await;
    let store = Arc::new(DatabaseProfileStore::new(pool.clone()));
    for id in ["one", "two", "three"] {
        store.record_turn_usage(update(id, 1_000)).await.unwrap();
    }
    let retention = ProfileUsageRetention::new(
        store,
        ProfileUsageRetentionConfig {
            cron: "0 0 0 * * *".to_string(),
            retention_days: 1,
            batch_size: 1,
            max_batches: 2,
        },
    );

    let now = DateTime::<Utc>::from_timestamp(2 * 24 * 60 * 60, 0).unwrap();
    assert_eq!(retention.run_at(now).await.unwrap(), 2);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM profile_turn_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1);
}
