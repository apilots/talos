use std::borrow::Cow;

use sqlx::Row;

use super::SQLITE_MIGRATOR;

#[tokio::test]
async fn workspace_scope_migration_discards_the_rebuildable_agent_index() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory index");
    let legacy_migrator = sqlx::migrate::Migrator {
        migrations: Cow::Owned(
            SQLITE_MIGRATOR
                .iter()
                .filter(|migration| migration.version < 202608290001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    legacy_migrator
        .run(&pool)
        .await
        .expect("legacy memory migrations");
    sqlx::query(
        r#"
        INSERT INTO memory_sources (
            agent_id, user_id, workspace_root, source_path, fingerprint
        ) VALUES ('agent-a', 'user-1', '/workspace/user-1', '2026-08-29.md', 'old')
        "#,
    )
    .execute(&pool)
    .await
    .expect("legacy index row");

    SQLITE_MIGRATOR
        .run(&pool)
        .await
        .expect("workspace scope migration");

    let columns = sqlx::query("PRAGMA table_info(memory_sources)")
        .fetch_all(&pool)
        .await
        .expect("memory source columns")
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect::<Vec<_>>();
    assert_eq!(
        columns,
        vec![
            "workspace_id",
            "user_id",
            "workspace_root",
            "source_path",
            "fingerprint",
        ]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM memory_sources")
            .fetch_one(&pool)
            .await
            .expect("derived row count"),
        0
    );
}
