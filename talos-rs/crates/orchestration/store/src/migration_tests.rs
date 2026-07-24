use sqlx::SqlitePool;

use super::run_migrations;

#[tokio::test]
async fn migration_creates_profile_usage_schema() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

    run_migrations(&pool).await.unwrap();
    run_migrations(&pool).await.unwrap();

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'profile_turn_usage'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);

    let index_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_profile_turn_usage_finalized_completed'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(index_count, 1);
}

#[tokio::test]
async fn migration_enforces_file_state_invariants() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();

    sqlx::query(
        r#"
        INSERT INTO files (
            file_id, owner_user_id, project_id, storage_id, object_key,
            original_filename, mime_type, size_bytes, status, metadata_json,
            created_at, updated_at
        ) VALUES (?, ?, NULL, ?, ?, ?, NULL, NULL, 'uploading', '{}', ?, ?)
        "#,
    )
    .bind("file-uploading")
    .bind("user-1")
    .bind("local")
    .bind("files/file-uploading/content.pdf")
    .bind("content.pdf")
    .bind(100_i64)
    .bind(100_i64)
    .execute(&pool)
    .await
    .unwrap();

    let invalid = sqlx::query(
        r#"
        INSERT INTO files (
            file_id, owner_user_id, project_id, storage_id, object_key,
            original_filename, mime_type, size_bytes, status, metadata_json,
            created_at, updated_at
        ) VALUES (?, ?, NULL, ?, ?, ?, NULL, NULL, 'ready', '{}', ?, ?)
        "#,
    )
    .bind("file-invalid")
    .bind("user-1")
    .bind("local")
    .bind("files/file-invalid/content.pdf")
    .bind("content.pdf")
    .bind(100_i64)
    .bind(100_i64)
    .execute(&pool)
    .await;

    assert!(invalid.is_err());
}
