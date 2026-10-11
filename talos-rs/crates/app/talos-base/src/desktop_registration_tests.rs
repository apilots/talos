use super::*;
use crate::desktop_application::DesktopApplication;
use talos_auth::{
    CloudSessionCache, CloudSessionCacheEntry, CloudSessionUserSnapshot, OpenModeUserService,
    UserType,
};
use talos_config::{Config, TalosPaths, YamlConfigStore};
use talos_service::AccountService;

struct DesktopFixture {
    home: tempfile::TempDir,
    config: Config,
    nodes: AgentNodeService,
    stop: CancellationToken,
    server: tokio::task::JoinHandle<TalosResult<()>>,
}

impl DesktopFixture {
    async fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let paths = TalosPaths::from_home(home.path().to_path_buf());
        paths.ensure_base_dirs().unwrap();
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let cert = paths.conf_dir.join("ca.crt");
        let key = paths.conf_dir.join("server.key");
        std::fs::write(&cert, certificate.cert.pem()).unwrap();
        std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
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
        let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let coordinator = Arc::new(CoordinatorImpl::new(
            Arc::new(talos_router::RouterImpl::new()),
            sessions.clone(),
            store,
        ));
        let server = NodeRpcServer::new(
            &talos_config::AgentNodeControlConfig {
                bind_address: address.to_string(),
                tls_certificate_file: cert.clone(),
                tls_private_key_file: key,
                trusted_node_ca_file: None,
            },
            nodes.clone(),
            Arc::new(TestAuth),
            coordinator,
            NodeSessionDataRpc::new(
                nodes.clone(),
                sessions,
                Arc::new(EmptyHistory),
                Arc::new(TestAttachments),
                Arc::new(TestMemoryProgress::default()),
            ),
            Arc::new(TestAgentControl),
            NodeToolImageRpc::new(nodes.clone(), Arc::new(TestToolImages), 1024),
        )
        .unwrap();
        let stop = CancellationToken::new();
        let server = tokio::spawn(server.run(stop.clone()));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while tokio::net::TcpStream::connect(address).await.is_err() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let mut config = Config::default().with_runtime_paths(paths);
        config.cloud.agent_base_url = format!("https://{address}");
        config.cloud.agent_trusted_ca_file = Some(cert);
        let mut persisted = serde_json::to_value(&config).unwrap();
        persisted["logging"].as_object_mut().unwrap().remove("file");
        persisted["storage"]
            .as_object_mut()
            .unwrap()
            .remove("connection_string");
        std::fs::write(
            config.source_path(),
            serde_json::to_string_pretty(&persisted).unwrap(),
        )
        .unwrap();
        Self {
            home,
            config,
            nodes,
            stop,
            server,
        }
    }

    fn login(&self, user_id: &str) {
        let now = CloudSessionCache::now_ms();
        CloudSessionCache::for_talos_home(self.home.path())
            .store(&CloudSessionCacheEntry {
                schema_version: 2,
                cloud_api_base_url: self.config.cloud.api_base_url.clone(),
                access_token: user_id.into(),
                token_type: "Bearer".into(),
                expires_at_ms: Some(now + 60_000),
                last_used_at_ms: now,
                user: CloudSessionUserSnapshot {
                    user_id: user_id.into(),
                    username: user_id.into(),
                    email: None,
                    role: UserRole::User,
                    r#type: UserType::Free,
                },
            })
            .unwrap();
    }

    fn host(&self, device_name: &str) -> DesktopApplication {
        let account = AccountService::new(
            Arc::new(TestAuth),
            Arc::new(OpenModeUserService),
            self.config.cloud.api_base_url.clone(),
        );
        account
            .set_cloud_session_cache(CloudSessionCache::for_talos_home(self.home.path()))
            .unwrap();
        DesktopApplication::new(
            self.config.clone(),
            account,
            Arc::new(TestAuth),
            device_name.into(),
            None,
        )
    }

    async fn registered(&self, node_id: &str) -> agent_protocol::NodeInfo {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let info = self
                    .nodes
                    .get_node(
                        &Actor::regular("user-1"),
                        agent_protocol::GetNodeRequest {
                            node_id: node_id.into(),
                        },
                    )
                    .await
                    .unwrap();
                if info.lease_expires_at.is_some() {
                    return info;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn desktop_enrollment_registers_reuses_renamed_uuid_and_recovers_credentials() {
    let fixture = DesktopFixture::new().await;
    let host = Arc::new(fixture.host("Original laptop"));
    assert!(host
        .enroll_worker(None)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Unauthenticated));
    fixture.login("user-1");
    let identity = host.enroll_worker(None).await.unwrap();
    let node_id = identity.node_id.clone().unwrap();
    use talos_app_server::AppServerApplication;
    let reply = host
        .dispatch(
            &Actor::regular("anonymous"),
            talos_app_protocol::requests::ClientRequest::decode("desktop/workerIdentity", None)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reply.body, serde_json::to_value(&identity).unwrap());
    let denied = host
        .dispatch(
            &Actor::regular("user-2").with_access_token("user-2"),
            talos_app_protocol::requests::ClientRequest::decode(
                "desktop/workerEnroll",
                Some(serde_json::json!({"nodeId": null})),
            )
            .unwrap(),
        )
        .await;
    assert!(matches!(denied, Err(error) if error.code == ErrorCode::Unauthenticated));
    let info = fixture.registered(&node_id).await;
    assert_eq!(info.display_name, "Original laptop");
    assert_eq!(info.endpoint, None);
    assert!(!info.available);
    assert!(!fixture.config.talos_paths().sqlite_db.exists());
    let yaml = std::fs::read_to_string(fixture.config.source_path()).unwrap();
    assert!(yaml.contains(&node_id));
    let saved = YamlConfigStore::new(fixture.config.source_path())
        .load_agent_node()
        .unwrap()
        .runner
        .unwrap();
    let secret = std::fs::read_to_string(&saved.credential_file).unwrap();
    assert!(!yaml.contains(&secret));
    assert!(!serde_json::to_string(&identity).unwrap().contains(&secret));
    // Exercise the actual bootstrap JSONL path, including omitted and explicit-null params.
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (mut client, server_io) = tokio::io::duplex(16 * 1024);
    let (reader, writer) = tokio::io::split(server_io);
    let server_task = tokio::spawn(talos_app_server::run_jsonl(
        reader,
        writer,
        host.clone(),
        Arc::new(talos_app_server::AuthServiceCredentialValidator::new(
            Arc::new(TestAuth),
            host.account.clone(),
        )),
        talos_app_server::AppServerConfig {
            auth_enabled: true,
            ..Default::default()
        },
    ));
    let initialize = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "client_info": { "name": "desktop", "version": "test", "kind": "desktop" },
        "capabilities": { "protocol_version": talos_app_protocol::version::ProtocolVersion::CURRENT, "supports_server_requests": false, "supports_event_replay": true }
    }});
    let frames = format!(
        "{initialize}\n{}\n{}\n",
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "desktop/connection/read"}),
        serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "desktop/workerIdentity", "params": null})
    );
    client.write_all(frames.as_bytes()).await.unwrap();
    client.shutdown().await.unwrap();
    let mut output = String::new();
    client.read_to_string(&mut output).await.unwrap();
    server_task.await.unwrap().unwrap();
    let responses: Vec<serde_json::Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let response = responses
        .iter()
        .find(|response| response["id"] == 3)
        .unwrap();
    assert_eq!(
        response["result"]["data"],
        serde_json::to_value(&identity).unwrap()
    );
    assert!(!output.contains(&secret));

    fixture
        .nodes
        .rename_computer(
            &Actor::regular("user-1"),
            talos_types::RenameComputerRequest {
                node_id: node_id.clone(),
                display_name: "User renamed laptop".into(),
            },
        )
        .await
        .unwrap();
    host.stop_worker().await.unwrap();
    let restarted = fixture.host("Changed hostname");
    assert_eq!(restarted.enroll_worker(None).await.unwrap(), identity);
    assert_eq!(
        fixture.registered(&node_id).await.display_name,
        "User renamed laptop"
    );
    restarted.stop_worker().await.unwrap();
    std::fs::remove_file(&saved.credential_file).unwrap();
    assert!(restarted.enroll_worker(None).await.is_err());
    assert_eq!(
        restarted
            .enroll_worker(Some(node_id.clone()))
            .await
            .unwrap(),
        identity
    );
    fixture.registered(&node_id).await;
    let cleared = restarted
        .dispatch(
            &Actor::regular("anonymous"),
            talos_app_protocol::requests::ClientRequest::decode("auth/session/clear", None)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cleared.body, serde_json::json!({"success": true}));
    assert!(CloudSessionCache::for_talos_home(fixture.home.path())
        .load_candidate(CloudSessionCache::now_ms())
        .unwrap()
        .is_none());
    assert!(fixture
        .nodes
        .get_node(
            &Actor::regular("user-1"),
            agent_protocol::GetNodeRequest {
                node_id: node_id.clone()
            }
        )
        .await
        .unwrap()
        .lease_expires_at
        .is_none());
    fixture.login("user-2");
    assert!(restarted.enroll_worker(None).await.is_err());
    let nodes = fixture
        .nodes
        .list_nodes(
            &Actor::regular("user-1"),
            agent_protocol::ListNodesRequest {
                cursor: None,
                limit: 100,
            },
        )
        .await
        .unwrap();
    assert_eq!(nodes.data.len(), 1);
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[tokio::test]
async fn desktop_observes_revoked_saved_registration_and_recovers_same_uuid() {
    use talos_app_server::AppServerApplication;
    let fixture = DesktopFixture::new().await;
    fixture.login("user-1");
    let first = fixture.host("Laptop");
    let identity = first.enroll_worker(None).await.unwrap();
    let node_id = identity.node_id.clone().unwrap();
    fixture.registered(&node_id).await;
    first.stop_worker().await.unwrap();
    fixture
        .nodes
        .rotate_credential(
            &Actor::regular("user-1"),
            agent_protocol::RotateCredentialRequest {
                node_id: node_id.clone(),
                expires_at: CloudSessionCache::now_ms() / 1000 + 3600,
            },
        )
        .await
        .unwrap();
    let restored = Arc::new(fixture.host("Changed hostname"));
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (client, peer) = tokio::io::duplex(16 * 1024);
    let (reader, mut requests) = tokio::io::split(client);
    let mut replies = tokio::io::BufReader::new(reader).lines();
    let (reader, writer) = tokio::io::split(peer);
    let host_task = tokio::spawn(talos_app_server::run_jsonl(
        reader,
        writer,
        restored.clone(),
        Arc::new(talos_app_server::AuthServiceCredentialValidator::new(
            Arc::new(TestAuth),
            restored.account.clone(),
        )),
        talos_app_server::AppServerConfig {
            auth_enabled: true,
            ..Default::default()
        },
    ));
    let initialize = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "client_info": { "name": "desktop", "version": "test", "kind": "desktop" },
        "capabilities": { "protocol_version": talos_app_protocol::version::ProtocolVersion::CURRENT, "supports_server_requests": false, "supports_event_replay": true }
    }});
    requests
        .write_all(format!("{initialize}\n").as_bytes())
        .await
        .unwrap();
    replies.next_line().await.unwrap().unwrap();
    let mut failures = restored.subscribe_worker_failures().unwrap();
    assert_eq!(restored.enroll_worker(None).await.unwrap(), identity);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while failures.borrow().is_none() {
            failures.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), replies.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let notification: serde_json::Value = serde_json::from_str(&notification).unwrap();
    assert_eq!(notification["method"], "desktop/worker/failed");
    let error: talos_error::ApiError =
        serde_json::from_value(notification["params"].clone()).unwrap();
    assert_eq!(error, failures.borrow().clone().unwrap());
    assert!(matches!(
        failures.borrow().as_ref().unwrap().code,
        ErrorCode::Unauthenticated | ErrorCode::PermissionDenied
    ));
    assert!(CloudSessionCache::for_talos_home(fixture.home.path())
        .load_candidate(CloudSessionCache::now_ms())
        .unwrap()
        .is_some());
    assert_eq!(
        restored.enroll_worker(Some(node_id)).await.unwrap(),
        identity
    );
    restored.stop_worker().await.unwrap();
    assert!(failures.borrow().is_none());
    requests.shutdown().await.unwrap();
    host_task.await.unwrap().unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[tokio::test]
async fn desktop_pending_first_enrollment_requires_explicit_owned_uuid() {
    let fixture = DesktopFixture::new().await;
    fixture.login("user-1");
    let created = fixture
        .nodes
        .create_node(
            &Actor::regular("user-1"),
            agent_protocol::CreateNodeRequest {
                display_name: "Lost response computer".into(),
                owner_user_id: Some("user-1".into()),
                endpoint: None,
                credential_expires_at: CloudSessionCache::now_ms() / 1000 + 3600,
            },
        )
        .await
        .unwrap();
    let store = YamlConfigStore::new(fixture.config.source_path());
    store.begin_node_enrollment(None).unwrap();
    let host = fixture.host("Renamed host");
    assert!(host
        .enroll_worker(None)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(store.load_agent_node().unwrap().runner.is_none());
    let identity = host
        .enroll_worker(Some(created.node.node_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        identity.node_id.as_deref(),
        Some(created.node.node_id.as_str())
    );
    assert!(!store.load_desktop().unwrap().enrollment_pending);
    fixture.registered(&created.node.node_id).await;
    host.stop_worker().await.unwrap();
    assert_eq!(
        fixture
            .nodes
            .list_nodes(
                &Actor::regular("user-1"),
                agent_protocol::ListNodesRequest {
                    cursor: None,
                    limit: 100
                }
            )
            .await
            .unwrap()
            .data
            .len(),
        1
    );
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}
