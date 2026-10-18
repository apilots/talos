use super::*;
use crate::Actor;
use std::sync::Arc;
use talos_auth::UserRole;
use talos_error::{ErrorCode, TalosError};
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;
use talos_types::{ComputerListRequest, DeleteComputerRequest};

async fn fixture() -> (
    SqliteStorage,
    AgentNodeService,
    talos_types::CreatedNode,
    tempfile::TempDir,
) {
    let home = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(home.path().join("test.db").to_str().unwrap())
        .await
        .unwrap();
    talos_store::migration::run_migrations(
        storage.pool(),
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let service = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(
        storage.pool().clone(),
    )));
    service.register_embedded(true).await.unwrap();
    let created = service
        .create_node(
            &Actor::regular("owner"),
            CreateNodeRequest {
                display_name: "Disposable Computer".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    (storage, service, created, home)
}

#[tokio::test]
async fn offline_deletion_is_owner_only_retryable_and_revokes_credentials() {
    let (storage, service, created, _home) = fixture().await;
    let request = DeleteComputerRequest {
        node_id: created.node.node_id.clone(),
    };
    for actor in [
        Actor::regular("other"),
        Actor::regular("admin").with_role(UserRole::Admin),
        Actor::auth_disabled(),
    ] {
        assert!(service
            .delete_computer(&actor, request.clone())
            .await
            .is_err());
    }
    let auth = agent_protocol::NodeAuth {
        node_id: request.node_id.clone(),
        token: created.credential.token,
    };
    service.authenticate_registration(&auth).await.unwrap();
    let owner = Actor::regular("owner");
    let response = service
        .delete_computer(&owner, request.clone())
        .await
        .unwrap();
    assert_eq!(
        response,
        talos_types::DeleteComputerResponse {
            node_id: request.node_id.clone()
        }
    );
    assert_eq!(
        service
            .delete_computer(&owner, request.clone())
            .await
            .unwrap(),
        response
    );
    assert!(service.authenticate_registration(&auth).await.is_err());
    assert!(service
        .list_computers(&owner, ComputerListRequest::default())
        .await
        .unwrap()
        .data
        .is_empty());
    assert_eq!(
        service
            .get_node(
                &owner,
                GetNodeRequest {
                    node_id: request.node_id.clone()
                }
            )
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::ResourceExpired
    );
    assert!(service
        .delete_computer(&Actor::regular("other"), request)
        .await
        .is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM computer_deletions")
        .fetch_one(storage.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert!(service
        .delete_computer(
            &owner,
            DeleteComputerRequest {
                node_id: "embedded".into()
            }
        )
        .await
        .is_err());
}

#[tokio::test]
async fn waiting_turn_prevents_deletion_and_failure_restores_worker_admission() {
    let (storage, service, created, _home) = fixture().await;
    let auth = agent_protocol::NodeAuth {
        node_id: created.node.node_id.clone(),
        token: created.credential.token,
    };
    let principal = service.authenticate_registration(&auth).await.unwrap();
    let lease = service
        .register(
            &principal,
            agent_protocol::RegisterParams {
                system_info: None,
                deployments: vec![agent_protocol::Deployment {
                    agent_id: "default".into(),
                }],
            },
        )
        .await
        .unwrap();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = calls.clone();
    service
        .set_registration_deletion_hook(
            &agent_protocol::NodeAuth {
                node_id: auth.node_id.clone(),
                token: lease.lease_token.clone(),
            },
            Arc::new(move |blocked| {
                observed.lock().unwrap().push(blocked);
                Box::pin(async { Ok(()) })
            }),
        )
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(session_key,agent_id,workspace_id,user_id,source_type,source_provider,environment_json,conf_json,usage_json,created_at,last_activity,execution_node_id) VALUES ('history','default','default','owner','http','http','null','{}','{}',1,1,?)")
        .bind(&auth.node_id).execute(storage.pool()).await.unwrap();
    sqlx::query("INSERT INTO agent_active_turns(session_id,turn_id,state,last_event_seq,updated_at) VALUES ('history','turn-1','waiting_for_input',1,1)").execute(storage.pool()).await.unwrap();
    let owner = Actor::regular("owner");
    let request = DeleteComputerRequest {
        node_id: auth.node_id.clone(),
    };
    assert_eq!(
        service
            .delete_computer(&owner, request.clone())
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::ActiveTurnRunning
    );
    assert_eq!(*calls.lock().unwrap(), vec![true, false]);
    service.authenticate_registration(&auth).await.unwrap();
    sqlx::query("DELETE FROM agent_active_turns")
        .execute(storage.pool())
        .await
        .unwrap();
    service.delete_computer(&owner, request).await.unwrap();
    assert!(service
        .authenticate_lease(&agent_protocol::NodeAuth {
            node_id: auth.node_id,
            token: lease.lease_token
        })
        .await
        .is_err());
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_key = 'history'")
            .fetch_one(storage.pool())
            .await
            .unwrap();
    assert_eq!(retained, 1);
}

#[tokio::test]
async fn expired_unreachable_publication_can_be_removed_but_busy_worker_cannot() {
    let (storage, service, created, _home) = fixture().await;
    let principal = service
        .authenticate_registration(&agent_protocol::NodeAuth {
            node_id: created.node.node_id.clone(),
            token: created.credential.token,
        })
        .await
        .unwrap();
    let lease = service
        .register(
            &principal,
            agent_protocol::RegisterParams {
                system_info: None,
                deployments: vec![agent_protocol::Deployment {
                    agent_id: "default".into(),
                }],
            },
        )
        .await
        .unwrap();
    let busy = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let observed = busy.clone();
    service
        .set_registration_deletion_hook(
            &agent_protocol::NodeAuth {
                node_id: created.node.node_id.clone(),
                token: lease.lease_token,
            },
            Arc::new(move |blocked| {
                let busy = observed.load(std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move {
                    if !blocked {
                        return Ok(());
                    }
                    if busy {
                        Err(TalosError::localized(
                            talos_error::ErrorLocalization::ComputerDeletionActive,
                        ))
                    } else {
                        Err(TalosError::new(
                            ErrorCode::Timeout,
                            "Worker connection timed out.",
                        ))
                    }
                })
            }),
        )
        .await
        .unwrap();
    let owner = Actor::regular("owner");
    let request = DeleteComputerRequest {
        node_id: created.node.node_id.clone(),
    };
    assert!(service
        .delete_computer(&owner, request.clone())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    busy.store(false, std::sync::atomic::Ordering::SeqCst);
    let unverified = service
        .delete_computer(&owner, request.clone())
        .await
        .unwrap_err();
    assert!(unverified.is_code(ErrorCode::ServiceUnavailable));
    assert_eq!(
        unverified.to_api_error().localization,
        Some(talos_error::ErrorLocalization::ComputerDeletionUnverified)
    );
    sqlx::query("UPDATE agent_nodes SET lease_expires_at = 1 WHERE node_id = ?")
        .bind(&request.node_id)
        .execute(storage.pool())
        .await
        .unwrap();
    busy.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(service
        .delete_computer(&owner, request.clone())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    busy.store(false, std::sync::atomic::Ordering::SeqCst);
    service.delete_computer(&owner, request).await.unwrap();
}
