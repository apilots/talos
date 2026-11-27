use super::*;
use talos_auth::{UserRole, UserType};
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;
use talos_types::{ComputerListRequest, GetNodeRequest};

async fn seeded_service() -> (AgentNodeService, ManagedWorkerRecord) {
    let storage = SqliteStorage::new_with_max_connections(":memory:", 1)
        .await
        .unwrap();
    crate::test_support::run_test_migrations(storage.pool()).await;
    let service = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
    let record = ManagedWorkerRecord {
        instance_id: "instance".into(),
        owner_user_id: "owner".into(),
        node_id: "managed-node".into(),
        conversation_session_id: "sess_talo".into(),
        short_id: "01234567".into(),
        image: "fixture".into(),
        revision: 1,
        generation: 1,
        desired: Desired::IdleSleep,
        operation: None,
        step: Step::Stable,
        home_ready: true,
        last_activity_at: now(),
        operation_at: now(),
        error: None,
    };
    service.store.reserve_managed_worker(&record).await.unwrap();
    (service, record)
}

#[tokio::test]
async fn managed_worker_management_and_discovery_require_cloud_admin_or_max() {
    let (service, record) = seeded_service().await;
    for actor in [
        Actor::regular("owner"),
        Actor::regular("owner").with_user_type(UserType::Plus),
        Actor::regular("owner").with_user_type(UserType::Pro),
        Actor::regular("owner")
            .with_role(UserRole::Admin)
            .with_auth_disabled(true),
    ] {
        let request = ManagedWorkerActionRequest {
            instance_id: record.instance_id.clone(),
            expected_generation: record.generation,
        };
        let actions = [
            service.read_managed_worker(&actor).await.map(|_| ()),
            service.create_managed_worker(&actor).await.map(|_| ()),
            service
                .start_managed_worker(&actor, request.clone())
                .await
                .map(|_| ()),
            service
                .stop_managed_worker(&actor, request.clone())
                .await
                .map(|_| ()),
            service
                .reset_managed_worker(&actor, request)
                .await
                .map(|_| ()),
            service
                .delete_managed_worker(
                    &actor,
                    DeleteManagedWorkerRequest {
                        instance_id: record.instance_id.clone(),
                        expected_generation: record.generation,
                        confirm_data_loss: true,
                    },
                )
                .await
                .map(|_| ()),
            service
                .acquire_managed_worker(&actor, &record.node_id)
                .await
                .map(|_| ()),
            service
                .read_computer(
                    &actor,
                    GetNodeRequest {
                        node_id: record.node_id.clone(),
                    },
                )
                .await
                .map(|_| ()),
        ];
        for result in actions {
            assert!(result.unwrap_err().is_code(ErrorCode::PermissionDenied));
        }
        let page = service
            .list_computers(
                &actor,
                ComputerListRequest {
                    cursor: None,
                    limit: Some(1),
                    query: None,
                },
            )
            .await
            .unwrap();
        assert!(page.data.is_empty());
        assert_eq!(page.total, 0);
    }
    for actor in [
        Actor::regular("owner").with_role(UserRole::Admin),
        Actor::regular("owner").with_user_type(UserType::Max),
    ] {
        assert_eq!(
            service
                .read_managed_worker(&actor)
                .await
                .unwrap()
                .instance
                .unwrap()
                .node_id,
            record.node_id
        );
        let page = service
            .list_computers(
                &actor,
                ComputerListRequest {
                    cursor: None,
                    limit: Some(10),
                    query: None,
                },
            )
            .await
            .unwrap();
        assert!(page.data.is_empty());
        assert_eq!(page.total, 0);
        let direct = service
            .read_computer(
                &actor,
                talos_types::GetNodeRequest {
                    node_id: record.node_id.clone(),
                },
            )
            .await
            .unwrap();
        assert!(direct.managed_worker_phase.is_some());
    }
    let persisted = service
        .store
        .managed_worker("owner")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.generation, 1);
    assert!(persisted.desired == Desired::IdleSleep);
}

struct UnusedRuntime;

impl ManagedWorkerRuntime for UnusedRuntime {
    fn ensure_home<'a>(
        &'a self,
        _: &'a WorkerSpec,
        _: HomeCapacity,
        _: &'a [talos_worker_runtime::SeedFile],
        _: talos_worker_runtime::SeedMode,
    ) -> talos_worker_runtime::RuntimeFuture<'a, ()> {
        panic!("authorization tests do not reconcile runtime resources")
    }

    fn ensure_runtime<'a>(
        &'a self,
        _: &'a WorkerSpec,
    ) -> talos_worker_runtime::RuntimeFuture<'a, ()> {
        panic!("authorization tests do not reconcile runtime resources")
    }

    fn set_running<'a>(
        &'a self,
        _: &'a WorkerSpec,
        _: DesiredRuntimeState,
    ) -> talos_worker_runtime::RuntimeFuture<'a, ()> {
        panic!("authorization tests do not reconcile runtime resources")
    }

    fn inspect<'a>(
        &'a self,
        _: &'a WorkerSpec,
    ) -> talos_worker_runtime::RuntimeFuture<'a, talos_worker_runtime::RuntimeObservation> {
        panic!("authorization tests do not reconcile runtime resources")
    }

    fn remove_runtime<'a>(
        &'a self,
        _: &'a WorkerSpec,
    ) -> talos_worker_runtime::RuntimeFuture<'a, ()> {
        panic!("authorization tests do not reconcile runtime resources")
    }

    fn remove_home<'a>(
        &'a self,
        _: &'a RuntimeIdentity,
    ) -> talos_worker_runtime::RuntimeFuture<'a, ()> {
        panic!("authorization tests do not reconcile runtime resources")
    }
}

#[derive(Default)]
struct ManagedAuth(std::sync::atomic::AtomicBool);

#[async_trait::async_trait]
impl talos_auth::AuthService for ManagedAuth {
    async fn logout(&self, _: &talos_auth::RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, _: &str) -> TalosResult<talos_auth::RequestIdentity> {
        Err(TalosError::authorization_denied())
    }

    fn has_verified_managed_worker_access(&self, user_id: &str) -> bool {
        user_id == "owner" && self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[tokio::test]
async fn managed_background_wake_requires_a_current_cloud_proof_before_changing_intent() {
    use std::sync::atomic::Ordering;
    let (service, record) = seeded_service().await;
    let auth = Arc::new(ManagedAuth::default());
    service
        .install_managed_worker_runtime(
            Arc::new(UnusedRuntime),
            auth.clone(),
            WorkerRuntimeConfig {
                kind: WorkerRuntimeKind::DockerEngine,
                image: "fixture".into(),
                control_plane_endpoint: "https://fixture.invalid".into(),
                trusted_ca_file: None,
                docker_engine: Default::default(),
                kubernetes: Default::default(),
            },
            ServerRuntimePolicy::default(),
            None,
        )
        .unwrap();
    let error = service
        .wake_for_managed_dispatch("owner", &record.node_id)
        .await
        .err()
        .unwrap();
    assert!(error.is_code(ErrorCode::PermissionDenied));
    let unchanged = service
        .store
        .managed_worker("owner")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.generation, record.generation);
    assert!(unchanged.desired == Desired::IdleSleep);
    auth.0.store(true, Ordering::SeqCst);
    assert_eq!(
        service
            .authorize_session_dispatch("owner", &record.node_id)
            .await
            .unwrap(),
        talos_types::SessionKind::Talo
    );
    assert!(
        service
            .wake_for_managed_dispatch("owner", &record.node_id)
            .await
            .unwrap()
            == ManagedDispatch::Waking
    );
    auth.0.store(false, Ordering::SeqCst);
    let error = service
        .wake_for_managed_dispatch("owner", &record.node_id)
        .await
        .err()
        .unwrap();
    assert!(error.is_code(ErrorCode::PermissionDenied));
    assert!(service
        .authorize_session_dispatch("owner", &record.node_id)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
}

#[tokio::test]
async fn managed_worker_publication_uses_its_lease_while_user_management_stays_restricted() {
    let (service, record) = seeded_service().await;
    service
        .store
        .rotate_credential(RotateCredentialParams {
            node_id: record.node_id.clone(),
            hash: hash_token("registration-credential"),
            expires_at: now() + 3600,
            now_at: now(),
        })
        .await
        .unwrap();
    service
        .store
        .set_status(talos_store::node::SetNodeStatusParams {
            node_id: record.node_id.clone(),
            status: NodeStatus::Enabled,
            now_at: now(),
        })
        .await
        .unwrap();
    let principal = service
        .authenticate_registration(&NodeAuth {
            node_id: record.node_id.clone(),
            token: "registration-credential".into(),
        })
        .await
        .unwrap();
    let registration = service
        .register(
            &principal,
            RegisterParams {
                system_info: None,
                deployments: vec![agent_protocol::Deployment {
                    agent_id: "talos".into(),
                }],
            },
        )
        .await
        .unwrap();
    let auth = NodeAuth {
        node_id: record.node_id.clone(),
        token: registration.lease_token,
    };
    assert_eq!(
        service
            .read_registered_computer(&auth)
            .await
            .unwrap()
            .node_id,
        record.node_id
    );
    assert!(service
        .get_node(
            &Actor::regular("owner"),
            GetNodeRequest {
                node_id: record.node_id.clone(),
            }
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    assert!(service
        .read_registered_computer(&NodeAuth {
            node_id: record.node_id,
            token: "invalid-lease".into(),
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::Unauthenticated));
}

#[tokio::test]
async fn managed_worker_info_reports_configured_idle_timeout_seconds() {
    let (service, _record) = seeded_service().await;
    let policy = ServerRuntimePolicy {
        idle_timeout_seconds: 600,
        ..Default::default()
    };
    service
        .install_managed_worker_runtime(
            Arc::new(UnusedRuntime),
            Arc::new(ManagedAuth::default()),
            WorkerRuntimeConfig {
                kind: WorkerRuntimeKind::DockerEngine,
                image: "fixture".into(),
                control_plane_endpoint: "https://fixture.invalid".into(),
                trusted_ca_file: None,
                docker_engine: Default::default(),
                kubernetes: Default::default(),
            },
            policy,
            None,
        )
        .unwrap();
    let actor = Actor::regular("owner").with_user_type(UserType::Max);
    let response = service.read_managed_worker(&actor).await.unwrap();
    let info = response.instance.unwrap();
    assert_eq!(info.idle_timeout_seconds, Some(600));
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["idleTimeoutSeconds"], 600);
    let roundtrip: ManagedWorkerInfo = serde_json::from_value(json).unwrap();
    assert_eq!(roundtrip.idle_timeout_seconds, Some(600));
}
