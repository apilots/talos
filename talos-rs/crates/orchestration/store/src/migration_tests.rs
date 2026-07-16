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
