use super::*;
use crate::runner::UpgradeRunner;
use crate::{run_startup_upgrades, StartupUpgradeParams};
use serde_json::Value;
use sqlx::Row;
use talos_config::{Config, TalosPaths};
use talos_sync_sdk::{version_equal, SyncReplica, SyncVersion};

fn fixtures() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../orchestration/sync-parser/testdata/released-v1")
}
fn migrations() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite")
}
async fn seed(storage: &SqliteStorage, account: &str) -> SyncVersion {
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(fixtures().join("survivor-b.json")).unwrap())
            .unwrap();
    let version: SyncVersion = serde_json::from_value(fixture["version"].clone()).unwrap();
    sqlx::query("INSERT INTO sync_bindings(hub_id,account_id,source_user_id,computers_json,download_cursor) VALUES ('retained-hub',?,?,'[]','old-feed')")
        .bind(account).bind(account).execute(storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO sync_documents(hub_id,account_id,kind,resource_id,computer_id,generation,deleted,snapshot_blob,version_vector,acknowledged_vector,acknowledged_generation,replica_schema_version) VALUES ('retained-hub',?,'session','released-session','d4383755-33d1-43fd-a6ef-cf14dc2a823d',0,0,?,?,?,0,1)")
        .bind(account).bind(std::fs::read(fixtures().join("survivor-b.bin")).unwrap())
        .bind(&version.version_vector).bind(&version.version_vector).execute(storage.pool()).await.unwrap();
    version
}
async fn checkpoint(storage: &SqliteStorage, account: &str) -> (Vec<u8>, Vec<u8>) {
    let row =
        sqlx::query("SELECT snapshot_blob,version_vector FROM sync_documents WHERE account_id=?")
            .bind(account)
            .fetch_one(storage.pool())
            .await
            .unwrap();
    (row.get("snapshot_blob"), row.get("version_vector"))
}
fn config(home: &std::path::Path) -> Config {
    let mut config = Config::default().with_runtime_paths(TalosPaths::from_home(home.into()));
    config.sync.enabled = Some(false);
    config
}
async fn initialize(storage: &SqliteStorage) {
    crate::database_migration::DatabaseMigrationRunner::new(storage.pool(), &migrations())
        .run()
        .await
        .unwrap();
}
#[tokio::test]
async fn startup_upgrades_dormant_accounts_preserves_pending_cuts_and_runs_once() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path());
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    initialize(&storage).await;
    let old_version = seed(&storage, "dormant-a").await;
    seed(&storage, "dormant-b").await;
    let pending: Value =
        serde_json::from_slice(&std::fs::read(fixtures().join("pending.json")).unwrap()).unwrap();
    // One real already-materialized prefix and one genuinely pending tail.
    for (changes, proof) in [
        (
            "[]".to_owned(),
            Some(serde_json::to_string(&old_version).unwrap()),
        ),
        (serde_json::to_string(&pending).unwrap(), None),
    ] {
        sqlx::query("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,exported_version_json,created_at) VALUES ('retained-hub','dormant-a','dormant-a','session','released-session',?,?,1)")
            .bind(changes).bind(proof).execute(storage.pool()).await.unwrap();
    }
    let report = run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap();
    assert!(report.product.applied.iter().any(|id| id == UPGRADE_ID));
    for account in ["dormant-a", "dormant-b"] {
        let (snapshot, _) = checkpoint(&storage, account).await;
        let replica = SyncReplica::from_snapshot(
            talos_sync_sdk::SyncResourceKey::new("session", "released-session"),
            &snapshot,
        )
        .unwrap();
        assert_eq!(
            replica
                .changes()
                .unwrap()
                .iter()
                .filter(|change| matches!(change, talos_sync_sdk::SyncChange::Stream(_)))
                .count(),
            4
        );
        let (schema, cursor, ack): (i64, Option<String>, Vec<u8>) = sqlx::query_as("SELECT replica_schema_version,b.download_cursor,acknowledged_vector FROM sync_documents d JOIN sync_bindings b USING(hub_id,account_id) WHERE d.account_id=?")
            .bind(account).fetch_one(storage.pool()).await.unwrap();
        assert_eq!((schema, cursor), (2, None));
        assert!(version_equal(
            &SyncVersion {
                version_vector: ack,
                history_generation: 0
            },
            &old_version
        )
        .unwrap());
    }
    let proofs: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT exported_version_json FROM sync_outbox WHERE account_id='dormant-a' ORDER BY seq",
    )
    .fetch_all(storage.pool())
    .await
    .unwrap();
    assert!(proofs[0].is_some());
    assert!(proofs[1].is_none());
    let pending_json: String = sqlx::query_scalar("SELECT changes_json FROM sync_outbox WHERE account_id='dormant-a' AND exported_version_json IS NULL")
        .fetch_one(storage.pool()).await.unwrap();
    assert!(serde_json::from_str::<Vec<talos_sync_sdk::SyncChange>>(&pending_json).is_ok());
    let before = checkpoint(&storage, "dormant-a").await;
    let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_outbox")
        .fetch_one(storage.pool())
        .await
        .unwrap();
    let retry = run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap();
    assert!(retry.product.skipped.iter().any(|id| id == UPGRADE_ID));
    assert_eq!(checkpoint(&storage, "dormant-a").await, before);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox")
            .fetch_one(storage.pool())
            .await
            .unwrap(),
        queued
    );
}
#[tokio::test]
async fn malformed_replica_aborts_startup_and_retries_after_repair() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path());
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    initialize(&storage).await;
    seed(&storage, "dormant").await;
    sqlx::query("UPDATE sync_documents SET snapshot_blob=x'1234'")
        .execute(storage.pool())
        .await
        .unwrap();
    let failed = run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await;
    assert!(
        failed.is_err(),
        "runtime assembly must not be authorized after conversion failure"
    );
    let state: Value = serde_json::from_slice(
        &std::fs::read(config.talos_paths().talos_home.join("upgrades/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["tasks"][UPGRADE_ID]["status"], "failed");
    let schema: i64 = sqlx::query_scalar("SELECT replica_schema_version FROM sync_documents")
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(schema, 1);
    sqlx::query("UPDATE sync_documents SET snapshot_blob=?")
        .bind(std::fs::read(fixtures().join("survivor-b.bin")).unwrap())
        .execute(storage.pool())
        .await
        .unwrap();
    run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT replica_schema_version FROM sync_documents")
            .fetch_one(storage.pool())
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn restored_v1_database_with_a_completed_newer_task_ledger_fails_closed() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path());
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    initialize(&storage).await;
    run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap();
    seed(&storage, "restored").await;
    let failure = run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap_err();
    assert!(failure.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT replica_schema_version FROM sync_documents")
            .fetch_one(storage.pool())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn restored_native_outbox_without_a_checkpoint_cannot_bypass_the_startup_guard() {
    let directory = tempfile::tempdir().unwrap();
    let config = config(directory.path());
    let storage = Arc::new(SqliteStorage::new("sqlite::memory:").await.unwrap());
    initialize(&storage).await;
    run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap();
    sqlx::query("INSERT INTO sync_bindings(hub_id,account_id,source_user_id,computers_json) VALUES ('retained-hub','restored','restored','[]')").execute(storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO sync_outbox(hub_id,account_id,source_user_id,kind,resource_id,changes_json,created_at) VALUES ('retained-hub','restored','restored','session','released-session',?,1)")
        .bind(std::fs::read_to_string(fixtures().join("pending.json")).unwrap()).execute(storage.pool()).await.unwrap();
    let failure = run_startup_upgrades(StartupUpgradeParams {
        config: &config,
        storage: &storage,
        database_migration_dir: &migrations(),
    })
    .await
    .unwrap_err();
    assert!(failure.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_documents")
            .fetch_one(storage.pool())
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sync_outbox")
            .fetch_one(storage.pool())
            .await
            .unwrap(),
        1
    );
}
