use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::CreateNodeRequest;
use talos_auth::{AuthService, RequestIdentity};
use talos_base::{NodeRpcServer, NodeSessionDataRpc, NodeToolImageRpc};
use talos_config::AgentNodeControlConfig;
use talos_coordinator::{AgentCoordinator, CoordinatorImpl};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{Actor, AgentNodeService};
use talos_storage::SqliteStorage;
use talos_store::node::{AgentNodeStore, DatabaseAgentNodeStore};
use talos_store::session::{DatabaseSessionStore, SessionStore};
use tokio_util::sync::CancellationToken;

struct RejectManagement;

#[tonic::async_trait]
impl AuthService for RejectManagement {
    async fn logout(&self, _: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, _: &str) -> TalosResult<RequestIdentity> {
        Err(TalosError::new(
            ErrorCode::Unauthenticated,
            "Invalid user token",
        ))
    }
}

struct NodeProcess(Child);

impl Drop for NodeProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn standalone_node_reregisters_after_control_restart_and_unregisters_on_signal() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("node-home");
    let conf = home.join("conf");
    std::fs::create_dir_all(&conf).unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let certificate_pem = certificate.cert.pem();
    let certificate_path = conf.join("control.crt");
    let private_key_path = conf.join("control.key");
    std::fs::write(&certificate_path, certificate_pem.as_bytes()).unwrap();
    std::fs::write(&private_key_path, certificate.signing_key.serialize_pem()).unwrap();

    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let control_config = AgentNodeControlConfig {
        bind_address: address.to_string(),
        tls_certificate_file: certificate_path.clone(),
        tls_private_key_file: private_key_path.clone(),
        trusted_node_ca_file: Some(certificate_path.clone()),
    };

    let node_socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let node_address = node_socket.local_addr().unwrap();
    drop(node_socket);
    let database = directory.path().join("control.sqlite");
    let storage = Arc::new(
        SqliteStorage::new(database.to_str().unwrap())
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let store = Arc::new(DatabaseAgentNodeStore::new(storage.pool().clone()));
    let sessions: Arc<dyn SessionStore> =
        Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(talos_router::RouterImpl::new()),
        sessions.clone(),
        store.clone(),
    ));
    let nodes = AgentNodeService::new(store.clone());
    let created = nodes
        .create_node(
            &Actor::regular("user-1"),
            CreateNodeRequest {
                display_name: "Laptop".into(),
                endpoint: Some(format!("https://{node_address}")),
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();

    let node_id = created.node.node_id;
    let credential_path = conf.join("node.credential");
    std::fs::write(&credential_path, created.credential.token).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&credential_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let config_path = conf.join("talos.yaml");
    std::fs::write(
        &config_path,
        format!(
            "workspaces:\n  default_workspace_id: default\n  definitions:\n    - workspace_id: default\n      root_prefix: workspace-talos\nagent:\n  name: talos\ngateway:\n  host: 127.0.0.1\n  port: 8080\n  auth: true\nllm:\n  provider: openai\n  model: test\nagent_node:\n  runner:\n    node_id: {node_id}\n    control_plane_endpoint: https://{address}\n    credential_file: {:?}\n    bind_address: {node_address}\n    tls_certificate_file: {:?}\n    tls_private_key_file: {:?}\n    trusted_ca_file: {:?}\n",
            credential_path.display().to_string(),
            certificate_path.display().to_string(),
            private_key_path.display().to_string(),
            certificate_path.display().to_string(),
        ),
    )
    .unwrap();

    let stop_control = CancellationToken::new();
    let control = control_server(
        &control_config,
        nodes.clone(),
        coordinator.clone(),
        sessions.clone(),
    );
    let control_task = tokio::spawn(control.run(stop_control.clone()));
    await_listener(address).await;

    let log_path = directory.path().join("node.log");
    let log = std::fs::File::create(&log_path).unwrap();
    let mut node = NodeProcess(
        Command::new(env!("CARGO_BIN_EXE_talos-worker"))
            .env("TALOS_HOME", &home)
            .env("TALOS_CONFIG", &config_path)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let first_hash = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(hash) = store.get(&node_id).await.unwrap().unwrap().lease_token_hash {
                break hash;
            }
            assert!(
                node.0.try_wait().unwrap().is_none(),
                "node exited: {}",
                std::fs::read_to_string(&log_path).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    coordinator.ensure_agent("default").await.unwrap();
    stop_control.cancel();
    control_task.await.unwrap().unwrap();
    let restarted = AgentNodeService::new(store.clone());
    let stop_restarted = CancellationToken::new();
    let control = control_server(&control_config, restarted, coordinator.clone(), sessions);
    let control_task = tokio::spawn(control.run(stop_restarted.clone()));
    await_listener(address).await;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let current = store.get(&node_id).await.unwrap().unwrap();
            if current
                .lease_token_hash
                .is_some_and(|hash| hash != first_hash)
            {
                break;
            }
            assert!(
                node.0.try_wait().unwrap().is_none(),
                "node exited: {}",
                std::fs::read_to_string(&log_path).unwrap()
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    #[cfg(unix)]
    unsafe {
        libc::kill(node.0.id() as i32, libc::SIGINT);
    }
    let status = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(status) = node.0.try_wait().unwrap() {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        status.success(),
        "node exited: {}",
        std::fs::read_to_string(&log_path).unwrap()
    );
    assert!(store
        .get(&node_id)
        .await
        .unwrap()
        .unwrap()
        .lease_token_hash
        .is_none());
    assert!(coordinator.ensure_agent("default").await.is_err());

    stop_restarted.cancel();
    control_task.await.unwrap().unwrap();
}

async fn await_listener(address: std::net::SocketAddr) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if tokio::net::TcpStream::connect(address).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

// This process scenario validates registration only. Resource calls must fail
// rather than returning placeholder data if execution accidentally starts.
struct UnexpectedResourceCall;

impl talos_agent::SessionHistoryReader for UnexpectedResourceCall {
    fn read(
        &self,
        _: talos_agent::SessionHistoryReadParams,
    ) -> talos_agent::AgentFuture<'_, TalosResult<Vec<talos_agent_api::SessionHistoryTurn>>> {
        unreachable!("registration must not read history")
    }
}

impl talos_agent::AttachmentReader for UnexpectedResourceCall {
    fn read<'a>(
        &'a self,
        _: &'a talos_agent::TurnFileScope,
        _: &'a [String],
    ) -> talos_agent::AgentFuture<'a, TalosResult<Vec<talos_agent::ResolvedAttachment>>> {
        unreachable!("registration must not read attachments")
    }
}

#[tonic::async_trait]
impl talos_agent_provider_kernel::KernelMemoryProgressStore for UnexpectedResourceCall {
    async fn processed_until_event_seq(&self, _: &str) -> TalosResult<u64> {
        unreachable!("registration must not read memory progress")
    }
    async fn advance_processed_until_event_seq(&self, _: &str, _: u64, _: u64) -> TalosResult<()> {
        unreachable!("registration must not update memory progress")
    }
}

impl talos_tools::ToolImageStore for UnexpectedResourceCall {
    fn read<'a>(
        &'a self,
        _: &'a str,
        _: &'a [String],
    ) -> talos_tools::AgentControlFuture<'a, Vec<talos_llm_provider::Attachment>> {
        unreachable!("registration must not read tool images")
    }
    fn save<'a>(
        &'a self,
        _: &'a str,
        _: Vec<talos_llm_provider::Attachment>,
    ) -> talos_tools::AgentControlFuture<'a, Vec<talos_types::FileInfo>> {
        unreachable!("registration must not save tool images")
    }
}

impl talos_tools::AgentControl for UnexpectedResourceCall {
    fn spawn(
        &self,
        _: talos_types::SpawnSubagentRequest,
    ) -> talos_tools::AgentControlFuture<'_, talos_types::SubagentSpawnResult> {
        unreachable!("registration must not spawn a subagent")
    }
    fn send_message(
        &self,
        _: talos_types::SendSubagentMessageRequest,
    ) -> talos_tools::AgentControlFuture<'_, talos_types::SendMessageResult> {
        unreachable!("registration must not send a subagent message")
    }
    fn wait(
        &self,
        _: talos_types::WaitSubagentRequest,
    ) -> talos_tools::AgentControlFuture<'_, talos_types::SubagentSnapshot> {
        unreachable!("registration must not wait for a subagent")
    }
    fn interrupt(
        &self,
        _: talos_types::InterruptSubagentRequest,
    ) -> talos_tools::AgentControlFuture<'_, talos_types::SubagentSnapshot> {
        unreachable!("registration must not interrupt a subagent")
    }
    fn follow_up(
        &self,
        _: talos_types::FollowUpSubagentRequest,
    ) -> talos_tools::AgentControlFuture<'_, talos_types::SubagentSnapshot> {
        unreachable!("registration must not follow up with a subagent")
    }
    fn list(
        &self,
        _: talos_types::ListSubagentsRequest,
    ) -> talos_tools::AgentControlFuture<'_, talos_types::AgentTreeSnapshot> {
        unreachable!("registration must not list subagents")
    }
}

fn control_server(
    config: &AgentNodeControlConfig,
    nodes: AgentNodeService,
    coordinator: Arc<CoordinatorImpl>,
    sessions: Arc<dyn SessionStore>,
) -> NodeRpcServer {
    let resources = Arc::new(UnexpectedResourceCall);
    let session_data = NodeSessionDataRpc::new(
        nodes.clone(),
        sessions,
        resources.clone(),
        resources.clone(),
        resources.clone(),
    );
    NodeRpcServer::new(
        config,
        nodes.clone(),
        Arc::new(RejectManagement),
        coordinator,
        session_data,
        resources.clone(),
        NodeToolImageRpc::new(nodes, resources, 1024),
    )
    .unwrap()
}
