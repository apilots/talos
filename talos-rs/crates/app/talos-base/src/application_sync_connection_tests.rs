//! Real SDK/TLS regressions for independent dials and lifecycle-bound publication.
use super::*;
use agent_pb as pb;
use std::sync::Arc;
use talos_coordinator::AgentCoordinator;
use talos_store::node::AgentNodeStore;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

#[derive(Clone)]
struct Gateway {
    entered: tokio::sync::mpsc::UnboundedSender<String>,
    closed: tokio::sync::mpsc::UnboundedSender<String>,
    release: Arc<tokio::sync::Semaphore>,
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
        _: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::DeleteComputerResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn create_node(
        &self,
        _: Request<pb::CreateNodeRequest>,
    ) -> Result<Response<pb::CreateNodeResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn get_node(
        &self,
        _: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn list_nodes(
        &self,
        _: Request<pb::ListNodesRequest>,
    ) -> Result<Response<pb::ListNodesResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn update_node(
        &self,
        _: Request<pb::UpdateNodeRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
    }
    async fn rename_computer(
        &self,
        _: Request<pb::RenameComputerRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        Err(Status::unimplemented("unused fixture RPC"))
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
        let service = Gateway {
            entered,
            closed,
            release: release.clone(),
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
        // These owned rows model the directory projection already imported from H;
        // this fixture exercises dialing/publication, not Worker enrollment.
        for node_id in ["slow", "fast", "resumed"] {
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
            gateway,
            auth,
            _home: home,
        }
    }

    async fn finish(self) {
        self.runtime.shutdown_sync().await.unwrap();
        self.runtime.worker_rpc.shutdown().await.unwrap();
        self.runtime.resource_rpc.shutdown().await.unwrap();
        self.runtime.automation.shutdown().await.unwrap();
        self.runtime.worker.shutdown().await.unwrap();
        self.gateway.abort();
        self.auth.abort();
    }
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
