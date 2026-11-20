use std::sync::Arc;
use talos_agent_api::{AgentEvent, AgentEventPayload, AgentTurnRef, AgentTurnStarted};
use talos_storage::SqliteStorage;

use super::{run_migrations, test_migrations_dir};
use crate::session::{AgentExecutionStore, DatabaseSessionStore};

#[tokio::test]
async fn event_generation_keeps_existing_cursor_and_allows_reuse_of_a_retired_turn_key() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    let pool = storage.pool();
    let migrations = sqlx::migrate::Migrator::new(test_migrations_dir())
        .await
        .unwrap();
    let previous = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            migrations
                .iter()
                .filter(|m| m.version < 202611180001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    previous.run(pool).await.unwrap();
    // A previously repaired installation has already retired the legacy table.
    sqlx::raw_sql(
        "DROP TRIGGER sessions_cleanup_after_delete;
         DROP TABLE legacy_timeline_events;
         CREATE TRIGGER sessions_cleanup_after_delete AFTER DELETE ON sessions
         BEGIN
             DELETE FROM agent_events WHERE session_id = OLD.session_key;
         END;",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sessions(session_key,user_id,agent_id,workspace_id,source_type,source_provider,environment_json,conf_json,usage_json,created_at,last_activity,history_generation)
         VALUES ('s','u','agent','workspace','http','http','null','{}','{}',1,1,7)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(r#"INSERT INTO agent_events(event_seq,session_id,turn_id,sequence,occurred_at,persisted_at,payload_json,execution_capture_json) VALUES (15,'s','t',1,1,1,'{"type":"turnStarted","data":{}}','{"private":true}')"#)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE sqlite_sequence SET seq=30 WHERE name='agent_events'")
        .execute(pool)
        .await
        .unwrap();
    let blocked = migrations.run(pool).await.unwrap_err();
    assert!(blocked
        .to_string()
        .contains("no such table: main.legacy_timeline_events"));
    let last_applied: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(last_applied, 202611180004);
    run_migrations(pool, &test_migrations_dir()).await.unwrap();
    run_migrations(pool, &test_migrations_dir()).await.unwrap();

    let effects = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let id = AgentTurnRef::new("s", "t");
    let original = effects.get_agent_event(15).await.unwrap().unwrap();
    assert_eq!(original.identity, id);
    let (generation, private): (i64, String) = sqlx::query_as(
        "SELECT history_generation, execution_capture_json FROM agent_events WHERE event_seq=15",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((generation, private.as_str()), (7, r#"{"private":true}"#));

    // The owner can retire a generation while retaining its source rows.
    sqlx::query("UPDATE sessions SET history_generation=8 WHERE session_key='s'")
        .execute(pool)
        .await
        .unwrap();
    assert!(effects.get_agent_event(15).await.unwrap().is_none());
    let reopened = effects
        .commit_maintenance_admission(AgentEvent {
            identity: id.clone(),
            sequence: 1,
            occurred_at: 2,
            payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        })
        .await
        .unwrap();
    let mut events = effects.list_agent_turn_events(&id, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events.pop().unwrap().event_seq, 31);
    assert_eq!(reopened.active_turn.last_event_seq, 31);
    let retained: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT event_seq, history_generation FROM agent_events WHERE session_id='s' ORDER BY event_seq",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(retained, [(15, 7), (31, 8)]);

    sqlx::query("INSERT INTO session_event_retention(session_id,through_generation,deleted) VALUES ('s',8,1)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sessions WHERE session_key='s'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sessions(session_key,user_id,agent_id,workspace_id,source_type,source_provider,environment_json,conf_json,usage_json,created_at,last_activity)
         VALUES ('s','u','agent','workspace','http','http','null','{}','{}',3,3)",
    )
    .execute(pool)
    .await
    .unwrap();
    assert!(effects
        .list_agent_turn_events(&id, 10)
        .await
        .unwrap()
        .is_empty());
    let fresh = effects
        .commit_maintenance_admission(AgentEvent {
            identity: id,
            sequence: 1,
            occurred_at: 3,
            payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        })
        .await
        .unwrap();
    let event_seq = fresh.active_turn.last_event_seq;
    let generation: i64 =
        sqlx::query_scalar("SELECT history_generation FROM agent_events WHERE event_seq=?")
            .bind(event_seq)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(generation, 9);
}
