use std::fs;

use super::*;

#[tokio::test]
async fn sqlx_ledger_skips_an_already_applied_migration() {
    let temp = tempfile::tempdir().expect("temp migration directory");
    fs::write(
        temp.path().join("202608290001_create_upgrade_probe.sql"),
        "CREATE TABLE upgrade_probe (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    let pool = SqlitePool::connect("sqlite::memory:")
        .await
        .expect("connect sqlite");

    let first = DatabaseMigrationRunner::new(&pool, temp.path())
        .run()
        .await
        .expect("apply migration");
    let second = DatabaseMigrationRunner::new(&pool, temp.path())
        .run()
        .await
        .expect("skip applied migration");

    assert_eq!(
        (
            first.applied_versions,
            first.previously_applied_count,
            first.total_applied_count,
            second.applied_versions,
            second.previously_applied_count,
            second.total_applied_count,
        ),
        (vec![202608290001], 0, 1, Vec::<i64>::new(), 1, 1)
    );
}

#[tokio::test]
async fn session_kind_upgrade_preserves_managed_ownership_and_captured_metadata() {
    let migrations =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    let all = sqlx::migrate::Migrator::new(migrations.clone())
        .await
        .unwrap();
    let prior = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|entry| entry.version < 202611210001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    prior.run(&pool).await.unwrap();
    sqlx::raw_sql(
        r#"INSERT INTO agent_nodes(node_id, owner_user_id, display_name, mode, credential_hash,
                credential_expires_at, created_at, updated_at)
           VALUES ('managed-node', 'user', 'Talo', 'remote', zeroblob(32), 10000, 1, 1);
           INSERT INTO managed_workers(instance_id, owner_user_id, node_id, short_id, image,
                revision, generation, desired, step, home_ready, last_activity_at, operation_at,
                conversation_session_id)
           VALUES ('instance', 'user', 'managed-node', '12345678', 'worker', 1, 1,
                'running', 'stable', 1, 1, 1, 'canonical');
           INSERT INTO managed_worker_deletions(instance_id, owner_user_id, node_id, generation)
           VALUES ('retired', 'user', 'retired-node', 1);
           INSERT INTO sync_bindings(hub_id, account_id, source_user_id, computers_json)
           VALUES ('hub', 'user', 'user', '[]');"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let cases = [
        ("ordinary", "embedded", "standard"),
        ("canonical", "managed-node", "talo"),
        ("additional", "managed-node", "talo"),
        ("retired-chat", "retired-node", "talo"),
    ];
    for (id, node, _) in cases {
        sqlx::query(
            "INSERT INTO sessions (session_key, agent_id, workspace_id, user_id, source_type,
             source_provider, execution_node_id, created_at, last_activity, environment_json,
             conf_json, usage_json)
             VALUES (?, 'agent', 'workspace', 'user', 'desktop', 'sdk', ?, 1, 2, 'null', '{}', '{}')",
        )
        .bind(id)
        .bind(node)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO resource_revisions(kind, resource_id, owner_user_id, execution_node_id, changes_json)
             VALUES ('session', ?, 'user', ?,
             '{"before":{"fields":{"title":"before"}},"after":{"fields":{"title":"after"}}}')"#,
        )
        .bind(id)
        .bind(node)
        .execute(&pool)
        .await
        .unwrap();
        let projection = serde_json::json!({
            "snapshot": {"session": {"id": id, "nodeId": node}}, "active_turns": []
        });
        sqlx::query(
            "INSERT INTO sync_documents(hub_id, account_id, kind, resource_id, generation, deleted,
             snapshot_blob, version_vector, session_projection_json)
             VALUES ('hub', 'user', 'session', ?, 0, 0, X'', X'', ?)",
        )
        .bind(id)
        .bind(projection.to_string())
        .execute(&pool)
        .await
        .unwrap();
        let snapshot = serde_json::json!({
            "kind": "session", "data": {"session": {"id": id, "nodeId": node}}
        });
        let changes = serde_json::json!([
            {"type": "metadata", "data": {"before": null, "after": snapshot}},
            {"type": "reset", "data": {"historyGeneration": 1}},
            {"type": "metadata", "data": {"before": snapshot, "after": snapshot}}
        ]);
        sqlx::query(
            "INSERT INTO sync_outbox(hub_id, account_id, source_user_id, kind, resource_id,
             changes_json, created_at) VALUES ('hub', 'user', 'user', 'session', ?, ?, 1)",
        )
        .bind(id)
        .bind(changes.to_string())
        .execute(&pool)
        .await
        .unwrap();
    }

    DatabaseMigrationRunner::new(&pool, &migrations)
        .run()
        .await
        .unwrap();
    for (id, node, kind) in cases {
        let stored: String =
            sqlx::query_scalar("SELECT session_kind FROM sessions WHERE session_key=?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, kind);
        let revision: String =
            sqlx::query_scalar("SELECT changes_json FROM resource_revisions WHERE resource_id=?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&revision).unwrap(),
            serde_json::json!({
                "before": {"fields": {"title": "before", "session_kind": kind}},
                "after": {"fields": {"title": "after", "session_kind": kind}}
            })
        );
        let projection: String = sqlx::query_scalar(
            "SELECT json_extract(session_projection_json, '$.snapshot.session.sessionKind')
             FROM sync_documents WHERE resource_id=?",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(projection, kind);
        let outbox: String =
            sqlx::query_scalar("SELECT changes_json FROM sync_outbox WHERE resource_id=?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let snapshot = serde_json::json!({
            "kind": "session",
            "data": {"session": {"id": id, "nodeId": node, "sessionKind": kind}}
        });
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&outbox).unwrap(),
            serde_json::json!([
                {"type": "metadata", "data": {"before": null, "after": snapshot}},
                {"type": "reset", "data": {"historyGeneration": 1}},
                {"type": "metadata", "data": {"before": snapshot, "after": snapshot}}
            ])
        );
    }
}
