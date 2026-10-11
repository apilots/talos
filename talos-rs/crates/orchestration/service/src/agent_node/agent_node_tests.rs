use std::path::Path;
use std::sync::Arc;

use agent_protocol::{Deployment, HeartbeatParams, NodeAuth, NodeHealth, RegisterParams};
use talos_auth::UserRole;
use talos_error::ErrorCode;
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;
use talos_store::session::{SessionRecord, SessionTreeIdentity};
use talos_types::talos::{SessionSourceProvider, SessionSourceType};
use talos_types::{SessionConf, SessionUsage};

use super::*;
use crate::Actor;

#[tokio::test]
async fn owner_registration_restart_and_credential_rotation() {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let store = Arc::new(DatabaseAgentNodeStore::new(storage.pool().clone()));
    let service = AgentNodeService::new(store.clone());
    let owner = Actor::regular("user-1");
    let other = Actor::regular("user-2");
    let administrator = Actor::regular("admin").with_role(UserRole::Admin);

    let created = service
        .create_node(
            &owner,
            CreateNodeRequest {
                display_name: "Laptop".into(),
                endpoint: Some("https://node.example:18801".into()),
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let node_id = created.node.node_id.clone();
    assert!(uuid::Uuid::parse_str(&node_id).is_ok());
    assert_eq!(created.node.owner_user_id.as_deref(), Some("user-1"));
    assert!(service
        .get_node(
            &other,
            GetNodeRequest {
                node_id: node_id.clone()
            }
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    assert!(service
        .list_nodes(
            &other,
            ListNodesRequest {
                cursor: None,
                limit: 50
            }
        )
        .await
        .unwrap()
        .data
        .is_empty());
    assert!(service
        .get_node(
            &administrator,
            GetNodeRequest {
                node_id: node_id.clone()
            }
        )
        .await
        .is_ok());

    let registration_auth = NodeAuth {
        node_id: node_id.clone(),
        token: created.credential.token,
    };
    let principal = service
        .authenticate_registration(&registration_auth)
        .await
        .unwrap();
    let registration = service
        .register(
            &principal,
            RegisterParams {
                deployments: vec![Deployment {
                    agent_id: "talos".into(),
                }],
            },
        )
        .await
        .unwrap();
    let lease_auth = NodeAuth {
        node_id: node_id.clone(),
        token: registration.lease_token.clone(),
    };
    assert_eq!(
        service.authorize_lease_owner(&lease_auth).await.unwrap(),
        "user-1"
    );
    service
        .authorize_session_lease(&lease_auth, &session_record(&node_id, "user-1"))
        .await
        .unwrap();
    assert!(service
        .authorize_session_lease(&lease_auth, &session_record("embedded", "user-1"))
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    assert!(service
        .authorize_session_lease(&lease_auth, &session_record(&node_id, "user-2"))
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    service
        .mark_registration_published(&lease_auth, Arc::new(|| Box::pin(async {})))
        .await
        .unwrap();
    let lease_principal = service.authenticate_lease(&lease_auth).await.unwrap();
    service
        .heartbeat(
            &lease_principal,
            HeartbeatParams {
                health: NodeHealth::Healthy,
            },
        )
        .await
        .unwrap();
    assert!(
        service
            .get_node(
                &owner,
                GetNodeRequest {
                    node_id: node_id.clone()
                }
            )
            .await
            .unwrap()
            .available
    );
    let restarted = AgentNodeService::new(store);
    let lease_principal = restarted.authenticate_lease(&lease_auth).await.unwrap();
    let heartbeat = restarted
        .heartbeat(
            &lease_principal,
            HeartbeatParams {
                health: NodeHealth::Healthy,
            },
        )
        .await
        .unwrap();
    assert!(heartbeat.registration_required);
    assert!(
        !restarted
            .get_node(
                &owner,
                GetNodeRequest {
                    node_id: node_id.clone()
                }
            )
            .await
            .unwrap()
            .available
    );

    restarted
        .rotate_credential(
            &owner,
            RotateCredentialRequest {
                node_id: node_id.clone(),
                expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    assert!(restarted
        .authenticate_lease(&lease_auth)
        .await
        .err()
        .unwrap()
        .is_code(ErrorCode::Unauthenticated));

    let embedded = restarted.register_embedded(true).await.unwrap();
    assert!(embedded.available);
    assert!(embedded.owner_user_id.is_none());
    assert!(
        restarted
            .get_node(
                &administrator,
                GetNodeRequest {
                    node_id: "embedded".into()
                }
            )
            .await
            .unwrap()
            .available
    );
    assert!(restarted
        .list_nodes(
            &owner,
            ListNodesRequest {
                cursor: None,
                limit: 50
            }
        )
        .await
        .unwrap()
        .data
        .iter()
        .all(|node| node.node_id != "embedded"));
    assert!(!restarted.register_embedded(false).await.unwrap().available);
    let embedded_rows = restarted
        .list_nodes(
            &administrator,
            ListNodesRequest {
                cursor: None,
                limit: 50,
            },
        )
        .await
        .unwrap()
        .data
        .into_iter()
        .filter(|node| node.node_id == "embedded")
        .count();
    assert_eq!(embedded_rows, 1);
}

fn session_record(execution_node_id: &str, user_id: &str) -> SessionRecord {
    SessionRecord {
        id: "session-1".into(),
        agent_id: "talos".into(),
        execution_node_id: execution_node_id.into(),
        tree_identity: SessionTreeIdentity::Primary,
        workspace_id: "default".into(),
        title: "Session".into(),
        user_id: user_id.into(),
        source_type: SessionSourceType::System,
        source_provider: SessionSourceProvider::System,
        channel_id: None,
        thread_id: None,
        environment: None,
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: SessionUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    }
}
