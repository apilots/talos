use std::path::Path;

use agent_client::{AgentNodeClient, AgentNodeRunner, AgentSessionDataClient};
use agent_pb as pb;
use agent_pb::agent_node_control_server::AgentNodeControl;
use agent_pb::agent_node_management_server::AgentNodeManagement;
use talos_agent::{
    AgentFuture, AgentResult, AttachmentReader, ResolvedAttachment, SessionHistoryReadParams,
    SessionHistoryReader, TurnFileScope,
};
use talos_agent_api::SessionHistoryTurn;
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_auth::{RequestIdentity, UserRole};
use talos_coordinator::AgentCoordinator;
use talos_error::TalosResult;
use talos_llm_provider::{Attachment, AttachmentKind};
use talos_storage::SqliteStorage;
use talos_store::node::{AgentNodeStore, DatabaseAgentNodeStore};
use talos_store::session::{
    DatabaseSessionStore, SessionRecord, SessionStore, SessionTreeIdentity,
};
use talos_tools::{AgentControl, AgentControlFuture, ToolImageStore};
use talos_types::talos::{SessionSourceProvider, SessionSourceType};
use talos_types::{
    AgentTreeSnapshot, FileInfo, FileScope, FollowUpSubagentRequest, InterruptSubagentRequest,
    ListSubagentsRequest, SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest,
    SubagentSnapshot, SubagentSpawnResult, WaitSubagentRequest,
};
use talos_types::{SessionConf, SessionUsage};
use tokio_util::sync::CancellationToken;

use super::*;

struct TestAuth;

struct EmptyHistory;

impl SessionHistoryReader for EmptyHistory {
    fn read(
        &self,
        _: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct TestAttachments;

impl AttachmentReader for TestAttachments {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>,
    > {
        Box::pin(async move {
            assert_eq!(scope.owner_user_id, "user-1");
            assert_eq!(scope.project_id.as_deref(), Some("project-1"));
            assert_eq!(file_ids, ["file-1"]);
            Ok(vec![ResolvedAttachment {
                file_id: "file-1".into(),
                filename: "input.txt".into(),
                media_type: "text/plain".into(),
                size_bytes: 7,
                bytes: bytes::Bytes::from_static(b"content"),
            }])
        })
    }
}

#[derive(Default)]
struct TestMemoryProgress(tokio::sync::Mutex<u64>);

#[async_trait::async_trait]
impl KernelMemoryProgressStore for TestMemoryProgress {
    async fn processed_until_event_seq(&self, _: &str) -> TalosResult<u64> {
        Ok(*self.0.lock().await)
    }

    async fn advance_processed_until_event_seq(
        &self,
        _: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        let mut current = self.0.lock().await;
        if *current != expected_until_seq {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "memory cursor conflict",
            ));
        }
        *current = next_until_seq;
        Ok(())
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
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(talos_router::RouterImpl::new()),
        Arc::new(DatabaseSessionStore::new(storage).await.unwrap()),
        store,
    ));
    let certificate = rcgen::generate_simple_self_signed(vec!["node.example".into()]).unwrap();
    let publisher = RemoteNodePublisher {
        coordinator: coordinator.clone(),
        nodes: nodes.clone(),
        trusted_node_ca: agent_client::AgentTlsTrust::Certificate(
            certificate.cert.pem().into_bytes(),
        ),
    };
    let management = NodeManagementRpc::new(nodes.clone(), Arc::new(TestAuth));
    let control = NodeControlRpc::new(nodes, publisher, CancellationToken::new());

    let created = management
        .create_node(management_request(
            "user-1",
            pb::CreateNodeRequest {
                display_name: "Laptop".into(),
                endpoint: Some("https://node.example:18801".into()),
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
    coordinator.ensure_agent("talos").await.unwrap();
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
    let store = Arc::new(DatabaseAgentNodeStore::new(storage.pool().clone()));
    let nodes = AgentNodeService::new(store.clone());
    let sessions = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let memory_progress: Arc<dyn KernelMemoryProgressStore> =
        Arc::new(TestMemoryProgress::default());
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
    let session_data = NodeSessionDataRpc::new(
        nodes.clone(),
        sessions.clone(),
        Arc::new(EmptyHistory),
        Arc::new(TestAttachments),
        memory_progress,
    );
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(talos_router::RouterImpl::new()),
        sessions,
        store.clone(),
    ));
    let server = NodeRpcServer::new(
        &talos_config::AgentNodeControlConfig {
            bind_address: address.to_string(),
            tls_certificate_file: certificate_path.clone(),
            tls_private_key_file: private_key_path,
            trusted_node_ca_file: Some(certificate_path),
        },
        nodes.clone(),
        Arc::new(TestAuth),
        coordinator.clone(),
        session_data,
        Arc::new(TestAgentControl),
        NodeToolImageRpc::new(nodes.clone(), Arc::new(TestToolImages), 1024),
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
                Ok(client) => break client,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
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
            resources_for_lease.update_auth(update.auth)?;
        }
        lease_events_for_observer.lock().unwrap().push(registered);
        Ok(())
    }));
    let node_task = tokio::spawn(runner.run(stop_node.clone()));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if lease_events.lock().unwrap().contains(&true) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    coordinator.ensure_agent("talos").await.unwrap();
    assert!(SessionHistoryReader::read(
        &resources,
        SessionHistoryReadParams {
            session_id: "session-1".into(),
            window: talos_agent::SessionHistoryWindow::Latest,
            limit: 10,
        }
    )
    .await
    .unwrap()
    .is_empty());
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
    assert_eq!(
        resources
            .processed_until_event_seq("session-1")
            .await
            .unwrap(),
        0
    );
    resources
        .advance_processed_until_event_seq("session-1", 0, 7)
        .await
        .unwrap();
    assert_eq!(
        resources
            .processed_until_event_seq("session-1")
            .await
            .unwrap(),
        7
    );
    assert!(resources
        .advance_processed_until_event_seq("session-1", 0, 8)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
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

fn session_record() -> SessionRecord {
    SessionRecord {
        id: "session-1".into(),
        agent_id: "talos".into(),
        execution_node_id: "laptop-1".into(),
        tree_identity: SessionTreeIdentity::Primary,
        workspace_id: "default".into(),
        title: "Session".into(),
        user_id: "user-1".into(),
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

#[path = "agent_node_lifecycle_tests.rs"]
mod lifecycle;

#[path = "desktop_registration_tests.rs"]
mod desktop_registration_tests;
