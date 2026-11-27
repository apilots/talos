//! Real SDK/TLS regressions for independent dials and lifecycle-bound publication.
use super::*;
use agent_pb as pb;
use std::sync::Arc;
use talos_coordinator::AgentCoordinator;
use talos_store::node::AgentNodeStore;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

fn actor() -> talos_service::Actor {
    talos_service::Actor::regular("account").with_access_token("credential")
}

#[derive(Clone)]
struct Gateway {
    entered: tokio::sync::mpsc::UnboundedSender<String>,
    closed: tokio::sync::mpsc::UnboundedSender<String>,
    release: Arc<tokio::sync::Semaphore>,
    directory: Arc<std::sync::RwLock<Vec<talos_types::NodeInfo>>>,
    block_directory: Arc<std::sync::atomic::AtomicBool>,
    block_response: Arc<std::sync::atomic::AtomicBool>,
    block_mutation: Arc<std::sync::atomic::AtomicBool>,
    fail_directory: Arc<std::sync::atomic::AtomicBool>,
    get_node_failure: Arc<std::sync::RwLock<Option<talos_error::TalosError>>>,
    created_response: Arc<std::sync::RwLock<Option<talos_types::NodeInfo>>>,
}

#[tonic::async_trait]
impl pb::agent_node_management_server::AgentNodeManagement for Gateway {
    async fn get_deployments(
        &self,
        _: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::GetDeploymentsResponse>, Status> {
        Ok(Response::new(pb::GetDeploymentsResponse {
            result: Some(pb::get_deployments_response::Result::Deployments(
                pb::NodeDeployments {
                    deployments: vec![pb::Deployment {
                        agent_id: "talos".into(),
                    }],
                },
            )),
        }))
    }

    async fn delete_computer(
        &self,
        request: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::DeleteComputerResponse>, Status> {
        let id = request.into_inner().node_id;
        self.directory
            .write()
            .unwrap()
            .retain(|node| node.node_id != id);
        Ok(Response::new(pb::DeleteComputerResponse {
            result: Some(pb::delete_computer_response::Result::NodeId(id)),
        }))
    }
    async fn create_node(
        &self,
        request: Request<pb::CreateNodeRequest>,
    ) -> Result<Response<pb::CreateNodeResponse>, Status> {
        let request = request.into_inner();
        if self
            .block_mutation
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.send("before-commit".into()).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        let mut node = directory_node("created");
        node.display_name = request.display_name;
        node.endpoint = request.endpoint;
        node.owner_user_id = Some(request.owner_user_id.unwrap_or_else(|| "account".into()));
        self.directory.write().unwrap().push(node.clone());
        if self
            .block_response
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.send("create".into()).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        let node = self
            .created_response
            .read()
            .unwrap()
            .clone()
            .unwrap_or(node);
        Ok(Response::new(pb::CreateNodeResponse {
            result: Some(pb::create_node_response::Result::Created(Box::new(
                pb::CreatedNode {
                    node: Some(agent_protocol::node_pb::encode_info(node)),
                    credential: Some(pb::NodeCredential {
                        node_id: "created".into(),
                        token: "fixture-credential".into(),
                        expires_at: request.credential_expires_at,
                    }),
                },
            ))),
        }))
    }
    async fn get_node(
        &self,
        request: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let node_id = request.into_inner().node_id;
        if let Some(error) = self.get_node_failure.read().unwrap().as_ref() {
            return Ok(Response::new(pb::GetNodeResponse {
                result: Some(pb::get_node_response::Result::Failure(Box::new(
                    agent_protocol::error_pb::encode_failure(error),
                ))),
            }));
        }
        let node = self
            .directory
            .read()
            .unwrap()
            .iter()
            .find(|node| node.node_id == node_id)
            .cloned();
        if node.is_some() || ["fast", "slow", "resumed", "created"].contains(&node_id.as_str()) {
            return Ok(Response::new(pb::GetNodeResponse {
                result: Some(match node {
                    Some(node) => pb::get_node_response::Result::Node(Box::new(
                        agent_protocol::node_pb::encode_info(node),
                    )),
                    None => pb::get_node_response::Result::Failure(Box::new(
                        agent_protocol::error_pb::encode_failure(
                            &talos_error::TalosError::localized(
                                talos_error::ErrorLocalization::ComputerRegistrationRemoved,
                            ),
                        ),
                    )),
                }),
            }));
        }
        if node_id.starts_with("status-") {
            self.entered.send(format!("status:{node_id}")).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        let owner = if node_id == "foreign" {
            "other"
        } else {
            "account"
        };
        let now = chrono::Utc::now().timestamp();
        let node = talos_types::NodeInfo {
            display_name: node_id.clone(),
            node_id,
            mode: talos_types::NodeMode::Remote,
            endpoint: None,
            status: talos_types::NodeStatus::Enabled,
            health: talos_types::NodeHealth::Healthy,
            lease_expires_at: Some(now + 60),
            last_heartbeat_at: Some(now),
            created_at: now,
            updated_at: now,
            owner_user_id: Some(owner.into()),
            available: true,
            credential_expires_at: None,
            managed_worker_phase: None,
            system_info: None,
        };
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(pb::get_node_response::Result::Node(Box::new(
                agent_protocol::node_pb::encode_info(node),
            ))),
        }))
    }
    async fn verify_saved_registration(
        &self,
        _: Request<pb::VerifySavedRegistrationRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        Err(Status::unimplemented("legacy Gateway"))
    }
    async fn list_nodes(
        &self,
        request: Request<pb::ListNodesRequest>,
    ) -> Result<Response<pb::ListNodesResponse>, Status> {
        let request = request.into_inner();
        assert!(request.owner_only);
        if self
            .fail_directory
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(Status::unavailable("controlled directory outage"));
        }
        let mut nodes = self.directory.read().unwrap().clone();
        nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        if self
            .block_directory
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.send("directory".into()).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        let nodes: Vec<_> = nodes
            .into_iter()
            .filter(|node| {
                request
                    .cursor
                    .as_ref()
                    .is_none_or(|cursor| node.node_id > *cursor)
            })
            .collect();
        let end = 2.min(nodes.len());
        let next_cursor = (end < nodes.len()).then(|| nodes[end - 1].node_id.clone());
        Ok(Response::new(pb::ListNodesResponse {
            result: Some(pb::list_nodes_response::Result::Page(pb::NodePage {
                data: nodes[..end]
                    .iter()
                    .cloned()
                    .map(agent_protocol::node_pb::encode_info)
                    .collect(),
                next_cursor,
            })),
        }))
    }

    async fn update_node(
        &self,
        _: Request<pb::UpdateNodeRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn rename_computer(
        &self,
        request: Request<pb::RenameComputerRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let request = request.into_inner();
        if self
            .block_mutation
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.send("before-commit".into()).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        let node = {
            let mut nodes = self.directory.write().unwrap();
            let node = nodes
                .iter_mut()
                .find(|node| node.node_id == request.node_id)
                .ok_or_else(|| Status::not_found("Computer"))?;
            node.display_name = request.display_name;
            node.updated_at += 1;
            node.clone()
        };
        if self
            .block_response
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.send("rename".into()).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(pb::get_node_response::Result::Node(Box::new(
                agent_protocol::node_pb::encode_info(node),
            ))),
        }))
    }
    async fn set_computer_status(
        &self,
        _: Request<pb::SetComputerStatusRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn rotate_credential(
        &self,
        _: Request<pb::RotateCredentialRequest>,
    ) -> Result<Response<pb::CredentialResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
}

#[tonic::async_trait]
impl pb::agent_coordinator_rpc_server::AgentCoordinatorRpc for Gateway {
    type ConnectStream = ReceiverStream<Result<pb::ControlToCoordinatorFrame, Status>>;

    async fn invoke_active_command(
        &self,
        _: Request<pb::ActiveSessionCommandRequest>,
    ) -> Result<Response<pb::ActiveSessionCommandResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }

    async fn connect(
        &self,
        request: Request<tonic::Streaming<pb::CoordinatorToControlFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let mut input = request.into_inner();
        let first = input.message().await?.unwrap();
        let Some(pb::coordinator_to_control_frame::Body::Connect(hello)) = first.body else {
            return Err(Status::invalid_argument("expected connection hello"));
        };
        self.entered.send(hello.computer_id.clone()).unwrap();
        if hello.computer_id == "slow" {
            self.release.acquire().await.unwrap().forget();
        }
        let (output, receiver) = tokio::sync::mpsc::channel(1);
        output
            .send(Ok(pb::ControlToCoordinatorFrame {
                body: Some(pb::control_to_coordinator_frame::Body::Connected(
                    pb::ConnectNodeResponse {
                        result: Some(pb::connect_node_response::Result::Ready(pb::Empty {})),
                        stream_heartbeat: false,
                    },
                )),
            }))
            .await
            .unwrap();
        let closed = self.closed.clone();
        tokio::spawn(async move {
            while input.message().await.is_ok_and(|frame| frame.is_some()) {}
            let _ = closed.send(hello.computer_id);
            drop(output);
        });
        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

struct Fixture {
    runtime: crate::TalosApplicationRuntime,
    identity: talos_auth::RequestIdentity,
    entered: tokio::sync::mpsc::UnboundedReceiver<String>,
    closed: tokio::sync::mpsc::UnboundedReceiver<String>,
    release: Arc<tokio::sync::Semaphore>,
    directory: Arc<std::sync::RwLock<Vec<talos_types::NodeInfo>>>,
    block_directory: Arc<std::sync::atomic::AtomicBool>,
    block_response: Arc<std::sync::atomic::AtomicBool>,
    block_mutation: Arc<std::sync::atomic::AtomicBool>,
    fail_directory: Arc<std::sync::atomic::AtomicBool>,
    get_node_failure: Arc<std::sync::RwLock<Option<talos_error::TalosError>>>,
    created_response: Arc<std::sync::RwLock<Option<talos_types::NodeInfo>>>,
    gateway: tokio::task::JoinHandle<()>,
    auth: tokio::task::JoinHandle<()>,
    _home: tempfile::TempDir,
}

impl Fixture {
    async fn start() -> Self {
        use axum::{routing::post, Json, Router};
        use serde_json::json;
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let home = tempfile::tempdir().unwrap();
        let auth_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = snapshot_config(home.path());
        config.sync.hub_endpoint = "wss://127.0.0.1:9/v1/sync".into();
        config.cloud.api_base_url = format!("http://{}", auth_listener.local_addr().unwrap());
        let auth = tokio::spawn(async move {
            axum::serve(auth_listener, Router::new().route("/v1/auth/introspect", post(|| async {
                Json(json!({"active":true,
                    "user":{"user_id":"account","email":"test@example.invalid","username":"test","role":"user","type":"free","status":"active"},
                    "capabilities":{"is_admin":false,"role":"user","scopes":[]},
                    "entitlement":{"can_use":true,"plan":"free","reason":null},"expires_at_ms":null}))
            }))).await.unwrap();
        });
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let cert = home.path().join("gateway.pem");
        std::fs::write(&cert, certificate.cert.pem()).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        config.cloud.agent_base_url = format!("https://{}", listener.local_addr().unwrap());
        config.cloud.agent_trusted_ca_file = Some(cert);
        let mut persisted = serde_json::to_value(&config).unwrap();
        persisted["logging"].as_object_mut().unwrap().remove("file");
        persisted["storage"]
            .as_object_mut()
            .unwrap()
            .remove("connection_string");
        std::fs::write(
            config.source_path(),
            serde_json::to_vec(&persisted).unwrap(),
        )
        .unwrap();
        let (entered, observed) = tokio::sync::mpsc::unbounded_channel();
        let (closed, observed_closed) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let directory = Arc::new(std::sync::RwLock::new(
            ["slow", "resumed", "fast"]
                .into_iter()
                .map(directory_node)
                .collect(),
        ));
        let block_directory = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let fail_directory = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let block_response = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let block_mutation = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let get_node_failure = Arc::new(std::sync::RwLock::new(None));
        let created_response = Arc::new(std::sync::RwLock::new(None));
        let service = Gateway {
            entered,
            closed,
            release: release.clone(),
            directory: directory.clone(),
            block_directory: block_directory.clone(),
            block_response: block_response.clone(),
            block_mutation: block_mutation.clone(),
            fail_directory: fail_directory.clone(),
            get_node_failure: get_node_failure.clone(),
            created_response: created_response.clone(),
        };
        let gateway = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(tonic::transport::ServerTlsConfig::new().identity(
                    tonic::transport::Identity::from_pem(
                        certificate.cert.pem(),
                        certificate.signing_key.serialize_pem(),
                    ),
                ))
                .unwrap()
                .add_service(
                    pb::agent_node_management_server::AgentNodeManagementServer::new(
                        service.clone(),
                    ),
                )
                .add_service(
                    pb::agent_coordinator_rpc_server::AgentCoordinatorRpcServer::new(service),
                )
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let runtime = crate::build_application_runtime_with_migration_dir(
            config,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
        )
        .await
        .unwrap();
        // Leave "fast" absent to exercise access before Sync imports the directory.
        for node_id in ["slow", "resumed"] {
            runtime
                .sync_host
                .sync_stores
                .nodes
                .create(talos_store::node::CreateNodeParams {
                    node_id: node_id.into(),
                    owner_user_id: Some("account".into()),
                    display_name: node_id.into(),
                    mode: agent_protocol::NodeMode::Remote,
                    endpoint: None,
                    credential_hash: None,
                    credential_expires_at: None,
                    now_at: chrono::Utc::now().timestamp(),
                })
                .await
                .unwrap();
        }
        let identity = runtime
            .sync_host
            .auth_service
            .verify_access_token("credential")
            .await
            .unwrap();
        Self {
            runtime,
            identity,
            entered: observed,
            closed: observed_closed,
            release,
            directory,
            block_directory,
            block_response,
            block_mutation,
            fail_directory,
            get_node_failure,
            created_response,
            gateway,
            auth,
            _home: home,
        }
    }

    async fn shutdown_runtime(&self) {
        self.runtime.shutdown_sync().await.unwrap();
        self.runtime.worker_rpc.shutdown().await.unwrap();
        self.runtime.resource_stop.cancel();
        self.runtime.automation.shutdown().await.unwrap();
        self.runtime.worker.shutdown().await.unwrap();
    }

    async fn restart(&mut self) {
        self.shutdown_runtime().await;
        self.runtime = crate::build_application_runtime_with_migration_dir(
            self.runtime.sync_host.config.clone(),
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
        )
        .await
        .unwrap();
    }

    async fn finish(self) {
        self.shutdown_runtime().await;
        self.gateway.abort();
        self.auth.abort();
    }
}

#[tokio::test]
async fn connection_admission_uses_server_authority_and_rejects_foreign_computers() {
    let mut fixture = Fixture::start().await;
    let nodes = &fixture.runtime.sync_host.sync_stores.nodes;
    let error = fixture
        .runtime
        .connect_computer(&fixture.identity, "foreign")
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::PermissionDenied));
    assert!(nodes.get("foreign").await.unwrap().is_none());
    assert!(fixture.entered.try_recv().is_err());

    nodes
        .reconcile_deleted_registration("deleted", "account")
        .await
        .unwrap();
    fixture
        .runtime
        .connect_computer(&fixture.identity, "deleted")
        .await
        .unwrap();
    assert_eq!(
        nodes
            .get("deleted")
            .await
            .unwrap()
            .unwrap()
            .owner_user_id
            .as_deref(),
        Some("account")
    );
    assert_eq!(fixture.entered.recv().await.unwrap(), "deleted");
    fixture.finish().await;
}

#[tokio::test]
async fn legacy_gateway_cannot_confirm_a_saved_registration() {
    let fixture = Fixture::start().await;
    let error = fixture
        .runtime
        .sync_host
        .gateway_client()
        .await
        .unwrap()
        .verify_saved_registration(
            "credential",
            agent_protocol::NodeAuth {
                node_id: "fast".into(),
                token: "saved-worker-credential".into(),
            },
        )
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::ServiceUnavailable));
    assert!(std::error::Error::source(&error).is_some());
    fixture.finish().await;
}

#[tokio::test]
async fn slow_computer_does_not_block_another_connection_or_logout() {
    let mut fixture = Fixture::start().await;
    let host = fixture.runtime.sync_host.clone();
    let identity = fixture.identity.clone();
    let slow = tokio::spawn(async move { host.connect_computer(&identity, "slow").await });
    assert_eq!(fixture.entered.recv().await.unwrap(), "slow");
    tokio::time::timeout(
        Duration::from_secs(2),
        fixture.runtime.connect_computer(&fixture.identity, "fast"),
    )
    .await
    .expect("another Computer must not wait for slow remote I/O")
    .unwrap();
    assert_eq!(fixture.entered.recv().await.unwrap(), "fast");
    assert!(fixture
        .runtime
        .coordinator
        .worker_environment("fast", "account")
        .await
        .is_ok());
    assert_eq!(
        fixture
            .runtime
            .coordinator
            .select_execution_node("talos", "account", Some("fast"))
            .await
            .unwrap(),
        "fast"
    );
    let actor = Actor::regular("account").with_access_token("credential");
    tokio::time::timeout(
        Duration::from_secs(2),
        fixture.runtime.service.suspend_account_sync(&actor),
    )
    .await
    .expect("logout must not wait for slow remote I/O")
    .unwrap();
    assert!(fixture
        .runtime
        .coordinator
        .worker_environment("fast", "account")
        .await
        .is_err());
    assert!(fixture
        .runtime
        .coordinator
        .select_execution_node("talos", "account", Some("fast"))
        .await
        .is_err());
    // A same-token login creates a new account lifetime; queued maintenance cannot
    // hide a stale publication by running another remote connection first.
    let lifecycle = fixture.runtime.sync_host.lifecycle.lock().await;
    fixture
        .runtime
        .service
        .resume_account_sync_after_login(&actor);
    fixture
        .runtime
        .connect_computer(&fixture.identity, "resumed")
        .await
        .unwrap();
    assert_eq!(fixture.entered.recv().await.unwrap(), "resumed");
    fixture.release.add_permits(1);
    let error = slow.await.unwrap().unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::Unauthenticated));
    assert!(fixture
        .runtime
        .sync_host
        .sync_stores
        .connections
        .lock()
        .await
        .get("slow")
        .is_none());
    assert!(fixture
        .runtime
        .coordinator
        .worker_environment("slow", "account")
        .await
        .is_err());
    assert!(fixture
        .runtime
        .coordinator
        .worker_environment("resumed", "account")
        .await
        .is_ok());
    drop(lifecycle);
    fixture.finish().await;
}

#[tokio::test]
async fn shutdown_rejects_a_late_computer_connection() {
    let mut fixture = Fixture::start().await;
    let host = fixture.runtime.sync_host.clone();
    let identity = fixture.identity.clone();
    let slow = tokio::spawn(async move { host.connect_computer(&identity, "slow").await });
    assert_eq!(fixture.entered.recv().await.unwrap(), "slow");
    tokio::time::timeout(Duration::from_secs(2), fixture.runtime.shutdown_sync())
        .await
        .expect("shutdown must not wait for slow remote I/O")
        .unwrap();
    fixture.release.add_permits(1);
    assert!(slow
        .await
        .unwrap()
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Unauthenticated));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), fixture.closed.recv())
            .await
            .unwrap()
            .unwrap(),
        "slow"
    );
    assert!(fixture
        .runtime
        .sync_host
        .sync_stores
        .connections
        .lock()
        .await
        .is_empty());
    assert!(fixture
        .runtime
        .coordinator
        .worker_environment("slow", "account")
        .await
        .is_err());
    fixture.finish().await;
}

#[path = "application_computer_directory_tests.rs"]
mod directory;

fn directory_node(id: &str) -> talos_types::NodeInfo {
    talos_types::NodeInfo {
        node_id: id.into(),
        display_name: format!("Server {id}"),
        owner_user_id: Some("account".into()),
        mode: talos_types::NodeMode::Remote,
        endpoint: None,
        status: talos_types::NodeStatus::Enabled,
        health: talos_types::NodeHealth::Healthy,
        available: true,
        lease_expires_at: Some(chrono::Utc::now().timestamp() + 60),
        last_heartbeat_at: Some(chrono::Utc::now().timestamp()),
        credential_expires_at: None,
        created_at: 3,
        updated_at: 4,
        managed_worker_phase: None,
        system_info: None,
    }
}

#[path = "application_sync_enrollment_binding_tests.rs"]
mod enrollment_binding;

#[path = "application_computer_confirmation_tests.rs"]
mod confirmation;
