use std::path::Path;

use agent_protocol::{NodeHealth, NodeMode, NodeStatus};
use talos_error::ErrorCode;
use talos_storage::SqliteStorage;

use super::*;

#[tokio::test]
async fn registration_survives_restart_and_stale_tokens_cannot_change_new_lease() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("agent-nodes.db");
    let database_path = database.to_str().unwrap();
    let storage = SqliteStorage::new(database_path).await.unwrap();
    crate::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let nodes = DatabaseAgentNodeStore::new(storage.pool().clone());
    let credential = [11; 32];
    let first_lease = [22; 32];
    let second_lease = [33; 32];

    nodes
        .create(CreateNodeParams {
            node_id: "laptop-1".into(),
            owner_user_id: Some("user-1".into()),
            display_name: "Laptop".into(),
            mode: NodeMode::Remote,
            endpoint: Some("https://node.example:18801".into()),
            credential_hash: Some(credential),
            credential_expires_at: Some(300),
            now_at: 10,
        })
        .await
        .unwrap();
    nodes
        .register(RegisterNodeParams {
            node_id: "laptop-1".into(),
            expected_credential_hash: credential,
            lease_token_hash: first_lease,
            lease_expires_at: 100,
            now_at: 10,
        })
        .await
        .unwrap();
    drop(nodes);
    drop(storage);

    let reopened = SqliteStorage::new(database_path).await.unwrap();
    let nodes = DatabaseAgentNodeStore::new(reopened.pool().clone());
    let previous = nodes.get("laptop-1").await.unwrap().unwrap();
    assert_eq!(previous.owner_user_id.as_deref(), Some("user-1"));
    assert_eq!(previous.lease_token_hash, Some(first_lease));

    nodes
        .register(RegisterNodeParams {
            node_id: "laptop-1".into(),
            expected_credential_hash: credential,
            lease_token_hash: second_lease,
            lease_expires_at: 120,
            now_at: 20,
        })
        .await
        .unwrap();
    let stale = LeaseCheck {
        node_id: "laptop-1".into(),
        token_hash: first_lease,
        now_at: 30,
    };
    assert!(nodes
        .unregister(stale)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceExpired));
    let current = nodes
        .heartbeat(HeartbeatNodeParams {
            lease: LeaseCheck {
                node_id: "laptop-1".into(),
                token_hash: second_lease,
                now_at: 30,
            },
            health: NodeHealth::Healthy,
        })
        .await
        .unwrap();
    assert_eq!(current.health, NodeHealth::Healthy);

    let updated = nodes
        .update(UpdateNodeParams {
            node_id: "laptop-1".into(),
            display_name: "Laptop".into(),
            endpoint: Some("https://node.example:18801".into()),
            status: NodeStatus::Disabled,
            now_at: 40,
        })
        .await
        .unwrap();
    assert_eq!(updated.lease_token_hash, None);
    assert_eq!(
        nodes
            .get("laptop-1")
            .await
            .unwrap()
            .unwrap()
            .lease_token_hash,
        None
    );
    assert_eq!(
        nodes
            .list(ListNodesParams {
                owner_user_id: Some("user-2".into()),
                cursor: None,
                limit: 50,
            })
            .await
            .unwrap()
            .data
            .len(),
        0
    );
}
