use talos_storage::SqliteStorage;
use talos_types::{ModelRef, ModelUsageKind, StopReason, TokenUsage};

use crate::migration::{run_migrations, test_migrations_dir};

use super::{
    DatabaseProfileStore, ModelCallUsageUpdate, ProfileModelUsageSummary, ProfileStore,
    ProfileUsageUpdate, TurnUsageUpdate,
};

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

fn model_call(request_id: &str, usage: TokenUsage) -> ModelCallUsageUpdate {
    ModelCallUsageUpdate {
        user_id: "user-1".to_string(),
        session_id: Some("session-1".to_string()),
        request_id: request_id.to_string(),
        kind: ModelUsageKind::Dream,
        model_ref: ModelRef {
            provider: "anthropic".to_string(),
            id: "claude".to_string(),
        },
        usage,
        usage_available: true,
        timezone: "UTC".to_string(),
        completed_at_ms: 2_000,
        usage_date: "2026-07-16".to_string(),
    }
}

#[tokio::test]
async fn model_call_usage_reuses_profile_aggregates_without_changing_session_count() {
    let store = store().await;
    store
        .record_turn_usage(update("turn-request", StopReason::EndTurn, usage(10, 2)))
        .await
        .unwrap();
    let dream = ProfileUsageUpdate::ModelCall(model_call("dream-request", usage(5, 1)));
    store.record_usage(dream.clone()).await.unwrap();
    store.record_usage(dream).await.unwrap();

    let summary = store.usage_summary("user-1").await.unwrap();
    assert_eq!(summary.usage, usage_with_cache(15, 3, 2, 4));
    assert_eq!(summary.usage_session_count, 1);
    let kinds = sqlx::query_scalar::<_, String>(
        "SELECT usage_kind FROM profile_turn_usage ORDER BY id ASC",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(kinds, vec!["turn".to_string(), "dream".to_string()]);
}

#[tokio::test]
async fn model_usage_limit_ranks_all_usage_kinds_by_total_tokens() {
    let store = store().await;
    for (index, token_count) in [(1, 10), (2, 20), (3, 30)] {
        let mut turn = update(
            &format!("turn-request-{index}"),
            StopReason::EndTurn,
            usage(token_count, 2),
        );
        turn.session_id = format!("session-{index}");
        turn.turn_id = format!("turn-{index}");
        turn.model_ref.id = format!("turn-model-{index}");
        store.record_turn_usage(turn).await.unwrap();
    }

    let mut dream = model_call("dream-request", usage(100, 5));
    dream.model_ref.id = "inner-model".to_string();
    store
        .record_usage(ProfileUsageUpdate::ModelCall(dream))
        .await
        .unwrap();

    assert_eq!(
        store.model_usage("user-1", 3).await.unwrap(),
        vec![
            ProfileModelUsageSummary {
                model_ref: ModelRef {
                    provider: "anthropic".to_string(),
                    id: "inner-model".to_string(),
                },
                turn_count: 0,
                usage: usage(100, 5),
                usage_available: true,
            },
            ProfileModelUsageSummary {
                model_ref: ModelRef {
                    provider: "anthropic".to_string(),
                    id: "turn-model-3".to_string(),
                },
                turn_count: 1,
                usage: usage(30, 2),
                usage_available: true,
            },
            ProfileModelUsageSummary {
                model_ref: ModelRef {
                    provider: "anthropic".to_string(),
                    id: "turn-model-2".to_string(),
                },
                turn_count: 1,
                usage: usage(20, 2),
                usage_available: true,
            },
        ]
    );
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
        .record_turn_usage(update(
            "request-2",
            StopReason::EndTurn,
            usage_with_cache(30, 5, 2, 4),
        ))
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
async fn duplicate_turn_snapshot_is_not_counted_twice() {
    let store = store().await;
    let waiting = update("request-1", StopReason::WaitingUserAction, usage(10, 2));

    store.record_turn_usage(waiting.clone()).await.unwrap();
    store.record_turn_usage(waiting).await.unwrap();
    store
        .record_turn_usage(update(
            "request-2",
            StopReason::EndTurn,
            usage_with_cache(30, 5, 2, 4),
        ))
        .await
        .unwrap();

    assert_eq!(
        store.usage_summary("user-1").await.unwrap().usage,
        usage_with_cache(30, 5, 2, 4)
    );
}

#[tokio::test]
async fn resume_without_new_usage_preserves_the_cumulative_snapshot() {
    let store = store().await;
    store
        .record_turn_usage(update(
            "request-1",
            StopReason::WaitingUserAction,
            usage(10, 2),
        ))
        .await
        .unwrap();

    store
        .record_turn_usage(update(
            "request-2",
            StopReason::WaitingUserAction,
            usage(10, 2),
        ))
        .await
        .unwrap();
    store
        .record_turn_usage(update("request-3", StopReason::EndTurn, usage(10, 2)))
        .await
        .unwrap();

    let summary = store.usage_summary("user-1").await.unwrap();
    assert_eq!(summary.usage, usage(10, 2));
    assert!(summary.usage_available);
}

#[tokio::test]
async fn turn_without_any_usage_remains_unavailable() {
    let store = store().await;
    let mut terminal = update("request-2", StopReason::EndTurn, TokenUsage::default());
    terminal.usage_available = false;
    store.record_turn_usage(terminal).await.unwrap();

    let summary = store.usage_summary("user-1").await.unwrap();
    assert_eq!(summary.usage, TokenUsage::default());
    assert!(!summary.usage_available);
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
