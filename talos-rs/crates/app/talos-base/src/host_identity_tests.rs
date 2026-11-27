use super::*;
use talos_storage::SqliteStorage;

#[tokio::test]
async fn host_reuses_existing_identity_and_writer_without_replication_runtime() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    storage.execute("CREATE TABLE sync_identity(singleton INTEGER PRIMARY KEY,peer_id TEXT NOT NULL,loro_writer_id TEXT NOT NULL)", &[]).await.unwrap();
    storage
        .execute(
            "INSERT INTO sync_identity VALUES (1,'existing-host','42')",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(open(&storage).await.unwrap(), "existing-host");
    let rows = storage
        .query("SELECT loro_writer_id FROM sync_identity", &[])
        .await
        .unwrap();
    assert_eq!(rows[0].get_string("loro_writer_id").as_deref(), Some("42"));
}

#[tokio::test]
async fn host_identity_survives_storage_reopen_without_a_second_identity() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("product.sqlite");
    let storage = SqliteStorage::new(path.to_str().unwrap()).await.unwrap();
    storage.execute("CREATE TABLE sync_identity(singleton INTEGER PRIMARY KEY,peer_id TEXT NOT NULL,loro_writer_id TEXT NOT NULL)", &[]).await.unwrap();
    let expected = open(&storage).await.unwrap();
    drop(storage);
    let reopened = SqliteStorage::new(path.to_str().unwrap()).await.unwrap();
    assert_eq!(open(&reopened).await.unwrap(), expected);
    assert_eq!(
        reopened
            .query("SELECT peer_id FROM sync_identity", &[])
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn capability_discovery_reports_assembled_modules_without_starting_them() {
    use tower::ServiceExt;
    for (owner, relay) in [
        (
            talos_types::NodeProductOwner::Computer,
            cfg!(feature = "relay-client"),
        ),
        (
            talos_types::NodeProductOwner::Server,
            cfg!(feature = "relay"),
        ),
    ] {
        let response = crate::product_capability_routes(owner)
            .oneshot(
                axum::http::Request::builder()
                    .uri("/v1/capabilities")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let response: talos_types::ApiResponse<talos_app_protocol::requests::ServerCapabilities> =
            serde_json::from_slice(&bytes).unwrap();
        assert!(
            matches!(response, talos_types::ApiResponse::Success { data }
            if data.sync == cfg!(feature = "sync") && data.relay == relay)
        );
    }
}
