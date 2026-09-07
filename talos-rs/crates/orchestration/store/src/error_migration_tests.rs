use super::{run_migrations, test_migrations_dir};
use talos_storage::SqliteStorage;

const VERSION: i64 = 202609060001;
const OLD: &str = r#"{"type":"turnFailed","data":{"error":{"code":"internalError","message":"SQL secret=/private/database password=sentinel","retryable":true}}}"#;

async fn legacy_pool(database: &str) -> sqlx::SqlitePool {
    let storage = SqliteStorage::new_with_max_connections(database, 1)
        .await
        .unwrap();
    let all = sqlx::migrate::Migrator::new(test_migrations_dir())
        .await
        .unwrap();
    let legacy = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|m| m.version < VERSION)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy.run(storage.pool()).await.unwrap();
    storage.pool().clone()
}

async fn insert(pool: &sqlx::SqlitePool, sequence: i64, payload: &str) {
    sqlx::query("INSERT INTO agent_events(session_id,turn_id,sequence,occurred_at,persisted_at,payload_json) VALUES ('s','t',?,1,1,?)")
        .bind(sequence).bind(payload).execute(pool).await.unwrap();
}

#[tokio::test]
async fn notice_upgrade_preserves_existing_message_blocks_and_event_positions() {
    let pool = legacy_pool(":memory:").await;
    let block = serde_json::json!({"blockId":"notice-1","status":"completed","startedAt":1,"completedAt":1,"content":{"type":"systemNotice","data":{"level":"warning","message":"Prompt context was truncated."}}});
    for (sequence, payload) in [
        serde_json::json!({"type":"messageBlockCompleted","data":{"messageId":"m","block":block}}),
        serde_json::json!({"type":"messageCompleted","data":{"messageId":"m","turnId":"t","role":"system","status":"completed","createdAt":1,"completedAt":1,"blocks":[block.clone(),block]}}),
    ].into_iter().enumerate() {
        insert(&pool, sequence as i64 + 1, &payload.to_string()).await;
    }
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    let records: Vec<(i64, String)> =
        sqlx::query_as("SELECT sequence,payload_json FROM agent_events ORDER BY event_seq")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        records.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        vec![1, 2]
    );
    for (_, payload) in records {
        let event: talos_agent_api::AgentEventPayload = serde_json::from_str(&payload).unwrap();
        let blocks = match event {
            talos_agent_api::AgentEventPayload::MessageBlockCompleted(completed) => {
                vec![completed.block]
            }
            talos_agent_api::AgentEventPayload::MessageCompleted(message) => message.blocks,
            _ => unreachable!(),
        };
        for block in blocks {
            assert_eq!(
                block.content,
                talos_agent_api::AgentMessageBlockContent::SystemNotice(
                    talos_agent_api::AgentSystemNotice {
                        level: talos_types::TalosTimelineNoticeLevel::Warning,
                        content: talos_agent_api::SystemNoticeContent::Text(
                            "Prompt context was truncated.".to_string()
                        )
                    }
                )
            );
        }
    }
}

#[tokio::test]
async fn migration_reuses_api_error_preserves_retry_and_runs_once() {
    let pool = legacy_pool(":memory:").await;
    insert(&pool, 1, OLD).await;
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    let payload: String = sqlx::query_scalar("SELECT payload_json FROM agent_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    let expected =
        talos_agent_api::AgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
            error: talos_error::ApiError::with_message(
                talos_error::ErrorCode::InternalError,
                "The previous model execution failed; legacy technical details were withheld.",
            ),
            retryable: true,
        });
    assert_eq!(
        serde_json::from_str::<talos_agent_api::AgentEventPayload>(&payload).unwrap(),
        expected
    );
    assert!(!payload.contains("sentinel"));
}

#[tokio::test]
async fn notice_migration_retains_deleted_cursor_high_water_and_rejects_invalid_notices() {
    for malformed in [false, true] {
        let pool = legacy_pool(":memory:").await;
        let payload = if malformed {
            r#"{"type":"messageBlockCompleted","data":{"block":{"content":{"type":"systemNotice","data":{"level":"warning","message":null}}}}}"#
        } else {
            r#"{"type":"turnStarted","data":{}}"#
        };
        insert(&pool, 1, payload).await;
        sqlx::query("UPDATE sqlite_sequence SET seq=100 WHERE name='agent_events'")
            .execute(&pool)
            .await
            .unwrap();
        let result = run_migrations(&pool, &test_migrations_dir()).await;
        assert_eq!(result.is_err(), malformed);
        if malformed {
            let saved: String = sqlx::query_scalar("SELECT payload_json FROM agent_events")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(saved, payload);
            let applied: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM _sqlx_migrations WHERE version=202609060002",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(applied, 0);
        } else {
            insert(&pool, 2, payload).await;
            let cursor: i64 = sqlx::query_scalar("SELECT MAX(event_seq) FROM agent_events")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(cursor, 101);
        }
    }
}

#[tokio::test]
async fn invalid_legacy_payload_rolls_back_all_rows_and_does_not_mark_version() {
    let pool = legacy_pool(":memory:").await;
    insert(&pool, 1, OLD).await;
    insert(&pool, 2, r#"{"type":"turnFailed","data":{"error":{"code":"unknownCode","message":"failure","retryable":false}}}"#).await;
    assert!(run_migrations(&pool, &test_migrations_dir()).await.is_err());
    let payload: String =
        sqlx::query_scalar("SELECT payload_json FROM agent_events WHERE sequence=1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(payload, OLD);
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version=?")
        .bind(VERSION)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(applied, 0);
}

#[tokio::test]
async fn pre_upgrade_sqlite_snapshot_restores_payload_and_migration_ledger() {
    let directory = tempfile::tempdir().unwrap();
    let backup = directory.path().join("before-upgrade.sqlite");
    let restored = directory.path().join("restored.sqlite");
    let active = directory.path().join("active.sqlite");
    let pool = legacy_pool(active.to_str().unwrap()).await;
    insert(&pool, 1, OLD).await;
    sqlx::query("VACUUM INTO ?")
        .bind(backup.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, &test_migrations_dir()).await.unwrap();
    pool.close().await;
    std::fs::copy(&backup, &restored).unwrap();
    let storage = SqliteStorage::new(restored.to_str().unwrap())
        .await
        .unwrap();
    let payload: String = sqlx::query_scalar("SELECT payload_json FROM agent_events")
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(payload, OLD);
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version=?")
        .bind(VERSION)
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(applied, 0);
}
