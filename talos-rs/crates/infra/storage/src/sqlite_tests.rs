use super::*;

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
