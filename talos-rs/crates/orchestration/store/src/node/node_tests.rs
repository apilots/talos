use std::path::Path;

use agent_protocol::{NodeHealth, NodeMode, NodeStatus};
use talos_error::ErrorCode;
use talos_storage::SqliteStorage;

use super::*;

#[tokio::test]
async fn outbound_migration_preserves_registered_nodes_and_security_constraints() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    let migrations = sqlx::migrate::Migrator::new(crate::migration::test_migrations_dir())
        .await
        .unwrap();
    sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            migrations
                .iter()
                .filter(|migration| migration.version < 202610110001)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    }
    .run(storage.pool())
    .await
    .unwrap();
    // Populate the previous schema through SQL; the current Store requires current migrations.
    sqlx::query("INSERT INTO agent_nodes(node_id, owner_user_id, display_name, mode, endpoint, status, credential_hash, credential_expires_at, lease_token_hash, lease_expires_at, health, created_at, updated_at) VALUES ('existing-node','owner','Existing laptop','remote','https://node.example','enabled',?,300,?,100,'unknown',10,10)")
        .bind([11u8;32].as_slice()).bind([22u8;32].as_slice()).execute(storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO agent_nodes(node_id, display_name, mode, status, health, created_at, updated_at) VALUES ('embedded','Embedded Agent','embedded','enabled','unknown',10,10)")
        .execute(storage.pool()).await.unwrap();
    let before = NodeRecord {
        system_info: None,
        node_id: "existing-node".into(),
        owner_user_id: Some("owner".into()),
        display_name: "Existing laptop".into(),
        mode: NodeMode::Remote,
        endpoint: Some("https://node.example".into()),
        status: NodeStatus::Enabled,
        credential_hash: Some([11; 32]),
        credential_expires_at: Some(300),
        lease_token_hash: Some([22; 32]),
        lease_expires_at: Some(100),
        last_heartbeat_at: None,
        health: NodeHealth::Unknown,
        created_at: 10,
        updated_at: 10,
    };
    crate::migration::run_migrations(storage.pool(), &crate::migration::test_migrations_dir())
        .await
        .unwrap();
    let nodes = DatabaseAgentNodeStore::new(storage.pool().clone());
    let after = nodes.get("existing-node").await.unwrap().unwrap();
    assert!(
        after == before,
        "migration must preserve the entire registered node"
    );
    assert_eq!(
        nodes.get("embedded").await.unwrap().unwrap().mode,
        NodeMode::Embedded
    );

    let outbound = |node_id: &str| CreateNodeParams {
        node_id: node_id.into(),
        owner_user_id: Some("owner".into()),
        display_name: "Outbound laptop".into(),
        mode: NodeMode::Remote,
        endpoint: None,
        credential_hash: Some([11; 32]),
        credential_expires_at: Some(300),
        now_at: 10,
    };
    assert_eq!(
        nodes
            .create(outbound("outbound-node"))
            .await
            .unwrap()
            .endpoint,
        None
    );
    for invalid in [
        CreateNodeParams {
            owner_user_id: None,
            ..outbound("missing-owner")
        },
        CreateNodeParams {
            credential_hash: None,
            ..outbound("missing-credential")
        },
        CreateNodeParams {
            endpoint: Some(" ".into()),
            ..outbound("empty-endpoint")
        },
    ] {
        assert!(nodes.create(invalid).await.is_err());
    }
    assert!(nodes
        .register(RegisterNodeParams {
            system_info: None,
            node_id: "outbound-node".into(),
            expected_credential_hash: [11; 32],
            lease_token_hash: [33; 32],
            lease_expires_at: 301,
            now_at: 10,
        })
        .await
        .is_err());
}

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
            system_info: Some(talos_types::NodeSystemInfo {
                os: talos_types::NodeOperatingSystem::MacOs,
                os_version: Some("macOS test".into()),
                architecture: "aarch64".into(),
                hostname: Some("test-laptop".into()),
                cpu_model: Some("Apple M2 Pro".into()),
                logical_cpu_count: 12,
                memory_bytes: 32 * 1024 * 1024 * 1024,
                board_model: None,
                network_interfaces: vec![],
                graphics: vec!["Apple M2 Pro".into()],
                worker_version: "test".into(),
            }),
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
    let report = previous
        .system_info
        .clone()
        .expect("registration report persisted");
    assert_eq!(report.cpu_model.as_deref(), Some("Apple M2 Pro"));

    nodes
        .register(RegisterNodeParams {
            system_info: None,
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
    assert_eq!(current.system_info, Some(report));

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
                exclude_node_id: None,
                scope: NodeListScope::Owner {
                    user_id: "user-2".into(),
                    include_embedded: false
                },
                query: None,
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
