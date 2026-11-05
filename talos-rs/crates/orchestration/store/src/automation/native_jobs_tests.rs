use super::*;
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn native_job_pause_survives_repository_reopen_and_stays_instance_scoped() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("jobs.sqlite");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&database)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../../conf/migrations/sqlite/202611040002_native_job_control.sql"
    )))
    .execute(&pool)
    .await
    .unwrap();
    let store = DatabaseNativeJobControlStore::new(pool.clone());
    assert!(store.load_paused("instance-a").await.unwrap().is_empty());
    store
        .set_paused(SetNativeJobPaused {
            execution_instance_id: "instance-a",
            job_id: BuiltinJobId::ProfileUsageRetention,
            paused: true,
        })
        .await
        .unwrap();
    drop(store);
    pool.close().await;
    let reopened_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap();
    let reopened = DatabaseNativeJobControlStore::new(reopened_pool);
    assert_eq!(
        reopened.load_paused("instance-a").await.unwrap(),
        vec![BuiltinJobId::ProfileUsageRetention]
    );
    assert!(reopened.load_paused("instance-b").await.unwrap().is_empty());
    reopened
        .set_paused(SetNativeJobPaused {
            execution_instance_id: "instance-b",
            job_id: BuiltinJobId::DreamMemoryMaintenance,
            paused: true,
        })
        .await
        .unwrap();
    reopened
        .set_paused(SetNativeJobPaused {
            execution_instance_id: "instance-a",
            job_id: BuiltinJobId::ProfileUsageRetention,
            paused: false,
        })
        .await
        .unwrap();
    assert!(reopened.load_paused("instance-a").await.unwrap().is_empty());
    assert_eq!(
        reopened.load_paused("instance-b").await.unwrap(),
        vec![BuiltinJobId::DreamMemoryMaintenance]
    );
    assert!(reopened
        .load_paused(" ")
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
}
