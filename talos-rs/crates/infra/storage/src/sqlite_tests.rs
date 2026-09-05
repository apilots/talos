use super::*;
use std::sync::Arc;
use tokio::sync::oneshot;

#[tokio::test]
async fn test_sqlite_storage() {
    let storage = SqliteStorage::new(":memory:").await.unwrap();

    storage
        .execute(
            "CREATE TABLE storage_test (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            &[],
        )
        .await
        .unwrap();

    let row = Row::new(
        vec!["id".to_string(), "value".to_string()],
        vec![Value::Number(1.into()), Value::String("stored".to_string())],
    );

    storage.insert("storage_test", &row).await.unwrap();

    let rows = storage
        .query("SELECT * FROM storage_test", &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_string("value").as_deref(), Some("stored"));
}

#[tokio::test]
async fn transaction_commits_or_rolls_back_all_writes() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    storage
        .execute(
            "CREATE TABLE transaction_test (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            &[],
        )
        .await
        .unwrap();

    let mut committed = storage.begin_transaction().await.unwrap();
    committed
        .execute(
            "INSERT INTO transaction_test (id, value) VALUES (?, ?)",
            &["1", "committed"],
        )
        .await
        .unwrap();
    assert_eq!(
        committed
            .query("SELECT value FROM transaction_test WHERE id = ?", &["1"])
            .await
            .unwrap()[0]
            .get_string("value")
            .as_deref(),
        Some("committed")
    );
    committed.commit().await.unwrap();

    let mut rolled_back = storage.begin_transaction().await.unwrap();
    rolled_back
        .execute(
            "INSERT INTO transaction_test (id, value) VALUES (?, ?)",
            &["2", "rolled-back"],
        )
        .await
        .unwrap();
    rolled_back.rollback().await.unwrap();

    let rows = storage
        .query("SELECT id FROM transaction_test ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get_i64("id"), Some(1));
}

#[tokio::test]
async fn dropped_transaction_rolls_back_without_losing_the_file_database() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("dropped-transaction.db");
    let storage = SqliteStorage::new_with_max_connections(database_path.to_str().unwrap(), 1)
        .await
        .unwrap();
    storage
        .execute(
            "CREATE TABLE dropped_transaction_test (id INTEGER PRIMARY KEY)",
            &[],
        )
        .await
        .unwrap();

    let mut transaction = storage.begin_transaction().await.unwrap();
    transaction
        .execute(
            "INSERT INTO dropped_transaction_test (id) VALUES (?)",
            &["1"],
        )
        .await
        .unwrap();
    drop(transaction);

    let rows = storage
        .query("SELECT id FROM dropped_transaction_test", &[])
        .await
        .unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn concurrent_writers_wait_for_the_active_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("concurrent-writers.db");
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(database_path.to_str().unwrap(), 2)
            .await
            .unwrap(),
    );
    storage
        .execute(
            "CREATE TABLE concurrent_writes (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            &[],
        )
        .await
        .unwrap();

    let mut first = storage.begin_transaction().await.unwrap();
    first
        .execute(
            "INSERT INTO concurrent_writes (id, value) VALUES (?, ?)",
            &["1", "first"],
        )
        .await
        .unwrap();

    let second_storage = storage.clone();
    let (started_tx, started_rx) = oneshot::channel();
    let second = tokio::spawn(async move {
        started_tx.send(()).unwrap();
        second_storage
            .execute(
                "INSERT INTO concurrent_writes (id, value) VALUES (?, ?)",
                &["2", "second"],
            )
            .await
    });
    started_rx.await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    first.commit().await.unwrap();

    second.await.unwrap().unwrap();
    let rows = storage
        .query("SELECT value FROM concurrent_writes ORDER BY id", &[])
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter_map(|row| row.get_string("value"))
            .collect::<Vec<_>>(),
        vec!["first".to_string(), "second".to_string()]
    );
}
