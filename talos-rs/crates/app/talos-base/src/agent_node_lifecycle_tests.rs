use super::*;
use talos_coordinator::AgentCoordinator;

struct Fixture {
    management: NodeManagementRpc,
    control: NodeControlRpc,
    publisher: RemoteNodePublisher,
    credential: pb::NodeCredential,
}

impl Fixture {
    async fn new() -> Self {
        let storage = Arc::new(
            SqliteStorage::new_with_max_connections(":memory:", 1)
                .await
                .unwrap(),
        );
        talos_store::migration::run_migrations(
            storage.pool(),
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
        )
        .await
        .unwrap();
        let store = Arc::new(DatabaseAgentNodeStore::new(storage.pool().clone()));
        let nodes = AgentNodeService::new(store.clone());
        let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let publisher = RemoteNodePublisher {
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            coordinator: Arc::new(CoordinatorImpl::new(
                Arc::new(talos_router::RouterImpl::new()),
                sessions,
                store,
            )),
            nodes: nodes.clone(),
            trusted_node_ca: Arc::new(certificate.cert.pem().into_bytes()),
        };
        let management =
            NodeManagementRpc::new(nodes.clone(), Arc::new(TestAuth), publisher.clone());
        let control = NodeControlRpc::new(nodes, publisher.clone());
        let created = management
            .create_node(management_request(
                "user-1",
                pb::CreateNodeRequest {
                    display_name: "Laptop".into(),
                    endpoint: "https://127.0.0.1:18801".into(),
                    owner_user_id: None,
                    credential_expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            ))
            .await
            .unwrap()
            .into_inner();
        let Some(pb::create_node_response::Result::Created(created)) = created.result else {
            panic!("create failed")
        };
        Self {
            management,
            control,
            publisher,
            credential: created.credential.unwrap(),
        }
    }

    fn registration(&self, agent_ids: &[&str]) -> Request<pb::RegisterRequest> {
        Request::new(pb::RegisterRequest {
            auth: Some(self.credential_auth()),
            params: Some(pb::RegisterParams {
                deployments: agent_ids
                    .iter()
                    .map(|id| pb::Deployment {
                        agent_id: (*id).into(),
                    })
                    .collect(),
            }),
        })
    }

    fn credential_auth(&self) -> pb::NodeAuth {
        pb::NodeAuth {
            node_id: self.credential.node_id.clone(),
            token: self.credential.token.clone(),
        }
    }

    async fn heartbeat(&self, token: String) {
        let response = self
            .control
            .heartbeat(Request::new(pb::HeartbeatRequest {
                auth: Some(pb::NodeAuth {
                    node_id: self.credential.node_id.clone(),
                    token,
                }),
                params: Some(pb::HeartbeatParams {
                    health: pb::NodeHealth::Healthy as i32,
                }),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(
            matches!(response.result, Some(pb::heartbeat_response::Result::Accepted(value)) if !value.registration_required)
        );
    }
}

fn registered(response: pb::RegisterResponse) -> String {
    let Some(pb::register_response::Result::Registered(value)) = response.result else {
        panic!("registration failed")
    };
    value.lease_token
}

#[tokio::test]
async fn rename_before_first_heartbeat_keeps_identity_and_executable_registration() {
    let fixture = Fixture::new().await;
    let empty = fixture
        .control
        .register(fixture.registration(&[]))
        .await
        .unwrap()
        .into_inner();
    assert!(
        matches!(empty.result, Some(pb::register_response::Result::Failure(value)) if value.error.as_ref().unwrap().code == pb::ErrorCode::InvalidRequest as i32)
    );
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    let renamed = fixture
        .management
        .update_node(management_request(
            "user-1",
            pb::UpdateNodeRequest {
                node_id: fixture.credential.node_id.clone(),
                display_name: "Renamed laptop".into(),
                endpoint: "https://127.0.0.1:18801".into(),
                status: pb::NodeStatus::Enabled as i32,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    let Some(pb::get_node_response::Result::Node(info)) = renamed.result else {
        panic!("update failed")
    };
    assert_eq!(info.node_id, fixture.credential.node_id);
    assert!(!info.available);
    fixture.heartbeat(lease).await;
    fixture
        .publisher
        .coordinator
        .ensure_agent("talos")
        .await
        .unwrap();
    assert!(
        fixture
            .publisher
            .nodes
            .get_node(
                &Actor::regular("user-1"),
                agent_protocol::GetNodeRequest {
                    node_id: info.node_id
                }
            )
            .await
            .unwrap()
            .available
    );
    let moved = fixture
        .management
        .update_node(management_request(
            "user-1",
            pb::UpdateNodeRequest {
                node_id: fixture.credential.node_id.clone(),
                display_name: "Renamed laptop".into(),
                endpoint: "https://127.0.0.1:18802".into(),
                status: pb::NodeStatus::Enabled as i32,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    let Some(pb::get_node_response::Result::Node(moved)) = moved.result else {
        panic!("endpoint update failed")
    };
    assert_eq!(moved.node_id, fixture.credential.node_id);
    assert!(moved.lease_expires_at.is_none());
    assert!(fixture
        .publisher
        .coordinator
        .ensure_agent("talos")
        .await
        .is_err());
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    fixture.heartbeat(lease).await;
    fixture
        .publisher
        .coordinator
        .ensure_agent("talos")
        .await
        .unwrap();
}

#[tokio::test]
async fn overlapping_unregister_and_register_keep_the_new_registration() {
    let fixture = Fixture::new().await;
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    let guard = fixture.publisher.lifecycle.lock().await;
    let mut unregister = Box::pin(fixture.control.unregister(Request::new(
        pb::UnregisterRequest {
            auth: Some(pb::NodeAuth {
                node_id: fixture.credential.node_id.clone(),
                token: lease.clone(),
            }),
            params: Some(pb::Empty {}),
        },
    )));
    let mut register = Box::pin(fixture.control.register(fixture.registration(&["talos"])));
    // Queue both public lifecycle calls in order, before either can mutate the directory.
    assert!(futures_util::poll!(&mut unregister).is_pending());
    assert!(futures_util::poll!(&mut register).is_pending());
    fixture
        .publisher
        .nodes
        .authenticate_lease(&NodeAuth {
            node_id: fixture.credential.node_id.clone(),
            token: lease,
        })
        .await
        .unwrap();
    drop(guard);
    let (removed, added) = tokio::join!(unregister, register);
    assert!(matches!(
        removed.unwrap().into_inner().result,
        Some(pb::unregister_response::Result::Completed(_))
    ));
    fixture
        .heartbeat(registered(added.unwrap().into_inner()))
        .await;
    fixture
        .publisher
        .coordinator
        .ensure_agent("talos")
        .await
        .unwrap();
}
