use super::*;
use std::sync::Arc;
use tokio::sync::oneshot;

#[tokio::test]
async fn read_snapshot_keeps_its_cut_and_releases_a_dropped_connection() {
    let home = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new_with_max_connections(
        home.path().join("snapshot.db").to_str().unwrap(),
        2,
    )
    .await
    .unwrap();
    storage
        .execute("CREATE TABLE snapshot_value (value INTEGER NOT NULL)", &[])
        .await
        .unwrap();
    storage
        .execute("INSERT INTO snapshot_value VALUES (1)", &[])
        .await
        .unwrap();
    let mut writer = storage.begin_transaction().await.unwrap();
    writer
        .execute("UPDATE snapshot_value SET value = 2", &[])
        .await
        .unwrap();
    let mut reader = tokio::time::timeout(Duration::from_secs(1), storage.begin_read_transaction())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reader
            .query("SELECT value FROM snapshot_value", &[])
            .await
            .unwrap()[0]
            .get_i64("value"),
        Some(1)
    );
    writer.commit().await.unwrap();
    assert_eq!(
        reader
            .query("SELECT value FROM snapshot_value", &[])
            .await
            .unwrap()[0]
            .get_i64("value"),
        Some(1)
    );
    assert_eq!(
        reader
            .execute("UPDATE snapshot_value SET value = 3", &[])
            .await
            .unwrap_err()
            .error_code(),
        talos_error::ErrorCode::InvalidRequest
    );
    drop(reader);
    let _connection = storage.pool.acquire().await.unwrap();
    let rows = tokio::time::timeout(
        Duration::from_secs(1),
        storage.query("SELECT value FROM snapshot_value", &[]),
    )
    .await
    .expect("dropping a read snapshot must release the other pooled connection")
    .unwrap();
    assert_eq!(rows[0].get_i64("value"), Some(2));
}

#[tokio::test]
async fn nullable_values_remain_absent_in_pool_and_transaction_reads() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    storage
        .execute(
            "CREATE TABLE nullable_values (optional_text TEXT, optional_number INTEGER, empty_text TEXT NOT NULL, zero_number INTEGER NOT NULL)",
            &[],
        )
        .await
        .unwrap();
    storage
        .execute(
            "INSERT INTO nullable_values VALUES (NULL, NULL, '', 0)",
            &[],
        )
        .await
        .unwrap();

    let assert_absent = |row: &Row| {
        assert_eq!(row.get("optional_text"), Some(&Value::Null));
        assert_eq!(row.get("optional_number"), Some(&Value::Null));
        assert_eq!(row.get_string("optional_text"), None);
        assert_eq!(row.get_i64("optional_number"), None);
        assert_eq!(row.get_string("empty_text").as_deref(), Some(""));
        assert_eq!(row.get_i64("zero_number"), Some(0));
    };
    let rows = storage
        .query("SELECT * FROM nullable_values", &[])
        .await
        .unwrap();
    assert_absent(&rows[0]);
    let mut transaction = storage.begin_transaction().await.unwrap();
    let rows = transaction
        .query("SELECT * FROM nullable_values", &[])
        .await
        .unwrap();
    assert_absent(&rows[0]);
    transaction.commit().await.unwrap();
}

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

#[tokio::test]
async fn cancelled_begin_does_not_return_an_open_transaction_or_retain_the_writer() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cancelled-begin.db");
    let storage = SqliteStorage::new_with_max_connections(path.to_str().unwrap(), 1)
        .await
        .unwrap();
    storage
        .execute("CREATE TABLE writes (id INTEGER PRIMARY KEY)", &[])
        .await
        .unwrap();
    let writer = SqliteWriteTransaction::begin(storage.pool()).await.unwrap();

    // Signal from SQLite's VM, rather than sleeping and guessing whether BEGIN
    // reached the worker thread. The first writer keeps this BEGIN pending.
    let started = Arc::new(tokio::sync::Notify::new());
    let progress = started.clone();
    let options = SqliteConnectOptions::from_str(&SqliteStorage::normalize_connection_string(
        path.to_str().unwrap(),
    ))
    .unwrap()
    .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .test_before_acquire(false)
        .after_connect(move |connection, _| {
            let progress = progress.clone();
            Box::pin(async move {
                connection
                    .lock_handle()
                    .await?
                    .set_progress_handler(1, move || {
                        progress.notify_one();
                        true
                    });
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap();
    let pending_pool = pool.clone();
    let pending = tokio::spawn(async move { SqliteWriteTransaction::begin(&pending_pool).await });
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    pending.abort();
    assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
    writer.commit().await.unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        let mut next = SqliteWriteTransaction::begin(&pool).await.unwrap();
        sqlx::query("INSERT INTO writes VALUES (1)")
            .execute(&mut *next)
            .await
            .unwrap();
        next.commit().await.unwrap();
        storage
            .execute("INSERT INTO writes VALUES (2)", &[])
            .await
            .unwrap();
    })
    .await
    .expect("cancelled BEGIN must release its connection and database writer");
    assert_eq!(
        storage
            .query("SELECT id FROM writes ORDER BY id", &[])
            .await
            .unwrap()
            .iter()
            .filter_map(|row| row.get_i64("id"))
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    pool.close().await;
}
