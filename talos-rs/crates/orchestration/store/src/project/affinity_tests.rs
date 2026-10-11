use super::{ProjectStore, StorageProjectStore};
use crate::{
    migration,
    session::{DatabaseSessionStore, MockSessionStore, SessionQuery, SessionStore},
};
use sqlx::Row;
use std::{path::Path, sync::Arc};
use talos_error::ErrorCode;
use talos_storage::SqliteStorage;

async fn storage_before_affinity() -> Arc<SqliteStorage> {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    let migrations = sqlx::migrate::Migrator::new(migration::test_migrations_dir())
        .await
        .unwrap();
    sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            migrations
                .iter()
                .filter(|migration| migration.version < 202610110002)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(storage.pool())
    .await
    .unwrap();
    storage
}
async fn history(storage: &Arc<SqliteStorage>, id: &str, node: &str) {
    let mut record = MockSessionStore::new()
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    record.id = id.into();
    record.user_id = "user-1".into();
    record.project_id = Some("project-1".into());
    record.execution_node_id = node.into();
    DatabaseSessionStore::new(storage.clone())
        .await
        .unwrap()
        .insert_session_record(&record)
        .await
        .unwrap();
}
async fn projects(storage: &SqliteStorage) {
    for (id, path) in [("project-1", "/project"), ("unused", "/unused")] {
        sqlx::query("INSERT INTO projects(project_id,name,user_id,directory,created_at,updated_at) VALUES (?, 'Project', 'user-1', ?, 1, 1)").bind(id).bind(path).execute(storage.pool()).await.unwrap();
    }
}

#[tokio::test]
async fn migration_uses_single_historical_node_and_leaves_session_binding_unchanged() {
    let storage = storage_before_affinity().await;
    projects(&storage).await;
    history(&storage, "session-1", "worker-a").await;
    migration::run_migrations(storage.pool(), &migration::test_migrations_dir())
        .await
        .unwrap();
    let store = StorageProjectStore::new(storage.clone()).await.unwrap();
    assert_eq!(
        store
            .get_project("project-1")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "worker-a"
    );
    assert_eq!(
        store
            .get_project("unused")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "embedded"
    );
    assert_eq!(
        DatabaseSessionStore::new(storage)
            .await
            .unwrap()
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        "worker-a"
    );
}

#[tokio::test]
async fn migration_rejects_multi_node_history_without_changing_schema_or_sessions() {
    let storage = storage_before_affinity().await;
    projects(&storage).await;
    history(&storage, "session-1", "worker-a").await;
    history(&storage, "session-2", "worker-b").await;
    assert!(
        migration::run_migrations(storage.pool(), &migration::test_migrations_dir())
            .await
            .is_err()
    );
    let columns = sqlx::query("PRAGMA table_info(projects)")
        .fetch_all(storage.pool())
        .await
        .unwrap();
    assert!(!columns
        .iter()
        .any(|row| row.get::<String, _>("name") == "execution_node_id"));
    let nodes: Vec<String> =
        sqlx::query_scalar("SELECT execution_node_id FROM sessions ORDER BY session_key")
            .fetch_all(storage.pool())
            .await
            .unwrap();
    assert_eq!(nodes, vec!["worker-a", "worker-b"]);
}

#[tokio::test]
async fn same_path_is_unique_per_owner_and_node_and_session_lists_filter_node() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    migration::run_migrations(storage.pool(), &migration::test_migrations_dir())
        .await
        .unwrap();
    let store = StorageProjectStore::new(storage.clone()).await.unwrap();
    let first = store
        .create_project_on_node("A", Path::new("/same"), "user-1", "worker-a")
        .await
        .unwrap();
    let second = store
        .create_project_on_node("B", Path::new("/same"), "user-1", "worker-b")
        .await
        .unwrap();
    assert_ne!(first.project_id, second.project_id);
    assert!(store
        .create_project_on_node("Duplicate", Path::new("/same"), "user-1", "worker-a")
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let sessions = DatabaseSessionStore::new(storage).await.unwrap();
    for (id, project) in [("session-a", first), ("session-b", second)] {
        let mut record = MockSessionStore::new()
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap();
        record.id = id.into();
        record.user_id = "user-1".into();
        record.project_id = Some(project.project_id);
        record.execution_node_id = project.execution_node_id;
        sessions.insert_session_record(&record).await.unwrap();
    }
    let query = SessionQuery {
        user_id: Some("user-1".into()),
        execution_node_id: Some("worker-b".into()),
        ..Default::default()
    };
    let records = sessions.list_session_records(&query).await.unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        vec!["session-b"]
    );
    assert_eq!(sessions.count_session_records(&query).await.unwrap(), 1);
}
