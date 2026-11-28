use crate::agent_node_resources_rpc::tests::{session_record, TestAttachments};
use std::path::Path;

use agent_client::{AgentNodeClient, AgentNodeRunner, AgentSessionDataClient};
use agent_pb as pb;
use agent_pb::agent_node_control_server::AgentNodeControl;
use agent_pb::agent_node_management_server::AgentNodeManagement;
use talos_agent::{AttachmentReader, TurnFileScope};
use talos_auth::{RequestIdentity, UserRole};
use talos_coordinator::AgentCoordinator;
use talos_error::TalosResult;
use talos_llm_provider::{Attachment, AttachmentKind};
use talos_storage::SqliteStorage;
use talos_store::node::{AgentNodeStore, DatabaseAgentNodeStore};
use talos_store::session::{DatabaseSessionStore, SessionRecord, SessionStore};
use talos_tools::{AgentControl, AgentControlFuture, ToolImageStore};
use talos_types::{
    AgentTreeSnapshot, FileInfo, FileScope, FollowUpSubagentRequest, InterruptSubagentRequest,
    ListSubagentsRequest, SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest,
    SubagentSnapshot, SubagentSpawnResult, WaitSubagentRequest,
};
use tokio_util::sync::CancellationToken;

use super::*;

struct TestAuth;

struct TestAutomationControl;

impl talos_tools::AutomationControl for TestAutomationControl {
    fn execute(
        &self,
        _: talos_types::AutomationModelRequest,
    ) -> talos_tools::AutomationControlFuture<'_> {
        Box::pin(async {
            Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Automation is not installed in this resource-only fixture",
            ))
        })
    }
}

struct TestAgentControl;

impl AgentControl for TestAgentControl {
    fn spawn(&self, _: SpawnSubagentRequest) -> AgentControlFuture<'_, SubagentSpawnResult> {
        Box::pin(async { Err(TalosError::internal("unused test operation")) })
    }

    fn send_message(
        &self,
        _: SendSubagentMessageRequest,
    ) -> AgentControlFuture<'_, SendMessageResult> {
        Box::pin(async { Err(TalosError::internal("unused test operation")) })
    }

    fn wait(&self, _: WaitSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(async { Err(TalosError::internal("unused test operation")) })
    }

    fn interrupt(&self, _: InterruptSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(async { Err(TalosError::internal("unused test operation")) })
    }

    fn follow_up(&self, _: FollowUpSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot> {
        Box::pin(async { Err(TalosError::internal("unused test operation")) })
    }

    fn list(&self, _: ListSubagentsRequest) -> AgentControlFuture<'_, AgentTreeSnapshot> {
        Box::pin(async { Ok(AgentTreeSnapshot { children: vec![] }) })
    }
}

struct TestToolImages;

impl ToolImageStore for TestToolImages {
    fn save<'a>(
        &'a self,
        user_id: &'a str,
        images: Vec<Attachment>,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<Vec<FileInfo>>> {
        Box::pin(async move {
            assert_eq!(user_id, "user-1");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].bytes, bytes::Bytes::from_static(b"image"));
            Ok(vec![FileInfo {
                file_id: "image-1".into(),
                scope: FileScope::Personal,
                original_filename: images[0].filename.clone(),
                mime_type: images[0].media_type.clone(),
                size_bytes: images[0].bytes.len() as u64,
                tags: vec!["mcp".into()],
                metadata: Default::default(),
                created_at: 1,
                updated_at: 1,
            }])
        })
    }

    fn read<'a>(
        &'a self,
        _: &'a str,
        _: &'a [String],
    ) -> futures_util::future::BoxFuture<'a, TalosResult<Vec<Attachment>>> {
        Box::pin(async { Err(TalosError::internal("unused test operation")) })
    }
}

#[tonic::async_trait]
impl AuthService for TestAuth {
    async fn logout(&self, _: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        let (user_id, role) = match token {
            "user-1" => ("user-1", UserRole::User),
            "user-2" => ("user-2", UserRole::User),
            "admin" => ("admin", UserRole::Admin),
            _ => {
                return Err(TalosError::new(
                    ErrorCode::Unauthenticated,
                    "User token is invalid",
                ))
            }
        };
        Ok(RequestIdentity {
            user_id: user_id.into(),
            username: None,
            role,
            user_type: Default::default(),
            auth_disabled: false,
            access_token: Some(token.into()),
        })
    }
}

fn management_request<T>(token: &str, body: T) -> Request<T> {
    let mut request = Request::new(body);
    request
        .metadata_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    request
}

#[tokio::test]
async fn rpc_adapter_enforces_user_owner_and_node_lease_boundaries() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
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
    let store = Arc::new(DatabaseAgentNodeStore::new(&storage));
    let nodes = AgentNodeService::new(store.clone());
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(talos_router::RouterImpl::new()),
        Arc::new(DatabaseSessionStore::new(storage).await.unwrap()),
        store,
    ));
    let publisher = RemoteNodePublisher {
        coordinator_id: uuid::Uuid::new_v4().to_string(),
        connections: Arc::default(),
        coordinator: coordinator.clone(),
    };
    let management = NodeManagementRpc::new(nodes.clone(), Arc::new(TestAuth));
    let control = NodeControlRpc::new(nodes.clone(), publisher.clone(), CancellationToken::new());

    let created = management
        .create_node(management_request(
            "user-1",
            pb::CreateNodeRequest {
                display_name: "Laptop".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    let credential = match created.result.unwrap() {
        pb::create_node_response::Result::Created(created) => created.credential.unwrap(),
        _ => panic!("node creation failed"),
    };

    let node_id = credential.node_id.clone();
    let credential = credential.token;
    let denied = management
        .get_node(management_request(
            "user-2",
            pb::GetNodeRequest {
                node_id: node_id.clone(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        denied.result,
        Some(pb::get_node_response::Result::Failure(failure))
            if failure.error.as_ref().is_some_and(|error| error.code == pb::ErrorCode::ResourceNotFound as i32)
    ));
    let node_as_user = management
        .get_node(management_request(
            &credential,
            pb::GetNodeRequest {
                node_id: node_id.clone(),
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        node_as_user.result,
        Some(pb::get_node_response::Result::Failure(failure))
            if failure.error.as_ref().is_some_and(|error| error.code == pb::ErrorCode::Unauthenticated as i32)
    ));

    let registered = control
        .register(Request::new(pb::RegisterRequest {
            auth: Some(pb::NodeAuth {
                node_id: node_id.clone(),
                token: credential,
            }),
            params: Some(pb::RegisterParams {
                product_owner: agent_pb::NodeProductOwner::Server as i32,
                system_info: None,
                deployments: vec![pb::Deployment {
                    agent_id: "talos".into(),
                }],
            }),
        }))
        .await
        .unwrap()
        .into_inner();
    let lease_token = match registered.result.unwrap() {
        pb::register_response::Result::Registered(registration) => registration.lease_token,
        _ => panic!("node registration failed"),
    };
    assert!(coordinator.ensure_agent("talos").await.is_err());
    let renewed = control
        .renew_lease(Request::new(pb::RenewLeaseRequest {
            auth: Some(pb::NodeAuth {
                node_id: node_id.clone(),
                token: lease_token.clone(),
            }),
            params: Some(pb::Empty {}),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        renewed.result,
        Some(pb::renew_lease_response::Result::Renewed(_))
    ));

    let rotated = management
        .rotate_credential(management_request(
            "admin",
            pb::RotateCredentialRequest {
                node_id: node_id.clone(),
                expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        ))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        rotated.result,
        Some(pb::credential_response::Result::Credential(_))
    ));
    assert!(coordinator.ensure_agent("talos").await.is_err());
    let stale = control
        .heartbeat(Request::new(pb::HeartbeatRequest {
            auth: Some(pb::NodeAuth {
                node_id: node_id.clone(),
                token: lease_token,
            }),
            params: Some(pb::HeartbeatParams {
                health: pb::NodeHealth::Healthy as i32,
            }),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        stale.result,
        Some(pb::heartbeat_response::Result::Failure(failure))
            if failure.error.as_ref().is_some_and(|error| error.code == pb::ErrorCode::Unauthenticated as i32)
    ));
}

#[tokio::test]
async fn tls_client_and_runner_register_then_unregister_over_grpc() {
    let directory = tempfile::tempdir().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let certificate_pem = certificate.cert.pem();
    let certificate_path = directory.path().join("control.crt");
    let private_key_path = directory.path().join("control.key");
    std::fs::write(&certificate_path, certificate_pem.as_bytes()).unwrap();
    std::fs::write(&private_key_path, certificate.signing_key.serialize_pem()).unwrap();

    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);

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
    let store = Arc::new(DatabaseAgentNodeStore::new(&storage));
    let nodes = AgentNodeService::new(store.clone());
    let sessions = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let created = nodes
        .create_node(
            &Actor::regular("user-1"),
            agent_protocol::CreateNodeRequest {
                display_name: "Laptop".into(),
                endpoint: Some(format!("https://{address}")),
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let node_id = created.node.node_id.clone();
    let mut session = session_record();
    session.execution_node_id = node_id.clone();
    sessions.insert_session_record(&session).await.unwrap();
    let session_data =
        NodeSessionDataRpc::new(nodes.clone(), sessions.clone(), Arc::new(TestAttachments));
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(talos_router::RouterImpl::new()),
        sessions.clone(),
        store.clone(),
    ));
    let server = NodeRpcServer::new(
        &talos_config::AgentNodeControlConfig {
            bind_address: address.to_string(),
            tls_certificate_file: certificate_path.clone(),
            tls_private_key_file: private_key_path,
        },
        nodes.clone(),
        Arc::new(TestAuth),
        coordinator.clone(),
        NodeResourceServices {
            session_data,
            agent_control: Arc::new(TestAgentControl),
            automation_control: Arc::new(TestAutomationControl),
            tool_images: NodeToolImageRpc::new(nodes.clone(), Arc::new(TestToolImages), 1024),
        },
    )
    .unwrap();
    let stop_server = CancellationToken::new();
    let server_task = tokio::spawn(server.run(stop_server.clone()));

    let client = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match AgentNodeClient::connect(
                &format!("https://{address}"),
                agent_client::AgentTlsTrust::Certificate(certificate_pem.as_bytes().to_vec()),
            )
            .await
            {
                Ok(client) if client.get_node("user-1", &node_id).await.is_ok() => break client,
                _ => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
            }
        }
    })
    .await
    .unwrap();
    let resources = AgentSessionDataClient::connect(
        &format!("https://{address}"),
        agent_client::AgentTlsTrust::Certificate(certificate_pem.as_bytes().to_vec()),
        agent_protocol::NodeAuth {
            node_id: node_id.clone(),
            token: created.credential.token.clone(),
        },
    )
    .await
    .unwrap();
    let lease_events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let resources_for_lease = resources.clone();
    let lease_events_for_observer = lease_events.clone();
    let worker_home = tempfile::tempdir().unwrap();
    let adapter = agent_client::AgentHarnessRpcAdapter::new(
        node_id.clone(),
        Arc::new(std::sync::RwLock::new(
            talos_config::Config::default().with_runtime_paths(
                talos_config::TalosPaths::from_home(worker_home.path().into()),
            ),
        )),
        Vec::new(),
    );
    let observed_adapter = adapter.clone();
    let stop_node = CancellationToken::new();
    let runner = AgentNodeRunner::new(
        client,
        node_id.clone(),
        created.credential.token,
        vec![agent_protocol::Deployment {
            agent_id: "talos".into(),
        }],
    )
    .with_lease_observer(Arc::new(move |update| {
        let registered = update.is_some();
        if let Some(update) = update {
            observed_adapter.install_registration(
                agent_protocol::Registration {
                    lease_token: update.auth.token.clone(),
                    validity: update.validity,
                    owner_user_id: update.owner_user_id,
                },
                update.deadline,
            )?;
            resources_for_lease.update_auth(update.auth)?;
        } else {
            observed_adapter.clear_registration();
        }
        lease_events_for_observer.lock().unwrap().push(registered);
        Ok(())
    }))
    .with_execution_adapter(adapter);
    let node_task = tokio::spawn(runner.run(stop_node.clone()));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if coordinator.ensure_agent("talos").await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    coordinator.ensure_agent("talos").await.unwrap();
    let attachments = AttachmentReader::read(
        &resources,
        &TurnFileScope {
            owner_user_id: "ignored-by-wire".into(),
            project_id: Some("project-1".into()),
        },
        &["file-1".into()],
    )
    .await
    .unwrap();
    assert_eq!(attachments[0].bytes, bytes::Bytes::from_static(b"content"));
    assert!(AgentControl::list(
        &resources,
        ListSubagentsRequest {
            caller: talos_types::AgentCaller {
                session_id: "session-1".into(),
                turn_id: "turn-1".into(),
                agent_id: "talos".into(),
            },
        },
    )
    .await
    .unwrap()
    .children
    .is_empty());
    let files = ToolImageStore::save(
        &resources,
        "ignored-by-wire",
        vec![Attachment::new(
            AttachmentKind::Image,
            "image.png",
            "image/png",
            bytes::Bytes::from_static(b"image"),
        )],
    )
    .await
    .unwrap();
    assert_eq!(files[0].file_id, "image-1");
    stop_node.cancel();
    node_task.await.unwrap().unwrap();
    assert_eq!(*lease_events.lock().unwrap(), vec![true, false]);
    assert!(store
        .get(&node_id)
        .await
        .unwrap()
        .unwrap()
        .lease_token_hash
        .is_none());
    assert!(coordinator.ensure_agent("talos").await.is_err());
    stop_server.cancel();
    server_task.await.unwrap().unwrap();
}

#[path = "agent_node_lifecycle_tests.rs"]
mod lifecycle;

#[path = "desktop_registration_tests.rs"]
mod desktop_registration_tests;

#[path = "agent_resource_automation_tests.rs"]
mod automation_resources;

#[path = "application_automation_worker_tests.rs"]
mod automation_worker_business;

#[path = "application_automation_skill_tests.rs"]
mod automation_skill_resolution;
