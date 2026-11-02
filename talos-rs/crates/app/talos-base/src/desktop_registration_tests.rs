use super::*;
use crate::desktop_application::DesktopApplication;
use agent_protocol::WorkerEnvironment;
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
    coordinator: Arc<CoordinatorImpl>,
    runtime: crate::TalosApplicationRuntime,
    stop: CancellationToken,
    server: tokio::task::JoinHandle<TalosResult<()>>,
}

impl DesktopFixture {
    async fn new() -> Self {
        // The actual Worker directory picker is confined to the process HOME.
        // Keep all product state in an owned temporary child of that boundary.
        let home = tempfile::Builder::new()
            .prefix(".talos-desktop-test-")
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap();
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
            coordinator.clone(),
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
        config.apply_derived_paths();
        config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
            reasoning_replay: Default::default(),
            reasoning_efforts: None,
            request_parameters: Default::default(),
            id: config.llm.model.clone(),
            name: config.llm.model.clone(),
            context_window: 32768,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
        }];
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
        let mut runtime = crate::build_application_runtime_with_migration_dir(
            config.clone(),
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
        )
        .await
        .unwrap();
        runtime.auth_service = Arc::new(TestAuth);
        runtime
            .service
            .account
            .set_cloud_session_cache(CloudSessionCache::for_talos_home(home.path()))
            .unwrap();
        Self {
            runtime,
            coordinator,
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
        let app = Arc::new(talos_app_server::TalosApplicationAdapter::new(
            self.runtime.service.clone(),
            128,
        ));
        let mut host = DesktopApplication::new(
            &self.runtime,
            device_name.into(),
            app,
            talos_app_server::DesktopHostBridge::new(),
        );
        host.account = account;
        host
    }

    async fn registered(&self, node_id: &str) -> agent_protocol::NodeInfo {
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
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
                if info.available {
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
    // A role or proximity alone never makes an authenticated user the Computer owner.
    for actor in [
        Actor::regular("user-1"),
        Actor::regular("administrator").with_role(UserRole::Admin),
    ] {
        let error = fixture
            .runtime
            .service
            .workspace_info(
                &actor,
                talos_types::WorkspaceInfoRequest {
                    node_id: Some("embedded".into()),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error.error_code(), ErrorCode::PermissionDenied);
    }
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
    assert!(info.available);
    assert!(fixture.config.talos_paths().sqlite_db.exists());
    let context = agent_protocol::WorkerEnvironmentContext {
        user_id: "user-1".into(),
        username: None,
        workspace_id: None,
    };
    let local = fixture
        .runtime
        .coordinator
        .worker_environment("embedded", "user-1")
        .await
        .unwrap();
    let remote = fixture
        .coordinator
        .worker_environment(&node_id, "user-1")
        .await
        .unwrap();
    let gateway = agent_client::AgentNodeClient::connect(
        &fixture.config.cloud.agent_base_url,
        agent_client::AgentTlsTrust::Certificate(
            std::fs::read(fixture.config.cloud.agent_trusted_ca_file.as_ref().unwrap()).unwrap(),
        ),
    )
    .await
    .unwrap();
    assert!(gateway
        .connect_coordinator("user-2", &uuid::Uuid::new_v4().to_string(), &node_id)
        .await
        .is_err());
    let (peer, auth) = gateway
        .connect_coordinator("user-1", &uuid::Uuid::new_v4().to_string(), &node_id)
        .await
        .unwrap();
    let caller = agent_client::AgentEnvironmentClient::from_connection(peer.clone(), auth);
    remote
        .create_directory(
            context.clone(),
            talos_types::FsDirectoryCreateRequest {
                node_id: None,
                parent: fixture.home.path().into(),
                name: "same-worker".into(),
            },
        )
        .await
        .unwrap();
    let directory = talos_types::FsDirectoryReadRequest {
        node_id: None,
        path: Some(fixture.home.path().join("same-worker")),
    };
    assert_eq!(
        local
            .read_directory(context.clone(), directory.clone())
            .await
            .unwrap(),
        remote
            .read_directory(context.clone(), directory.clone())
            .await
            .unwrap()
    );
    assert_eq!(
        caller
            .read_directory(context.clone(), directory.clone())
            .await
            .unwrap(),
        local
            .read_directory(context.clone(), directory.clone())
            .await
            .unwrap()
    );
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
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "desktop/workerIdentity"}),
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
    host.stop_registration().await.unwrap();
    assert!(!host.rpc.is_finished());
    assert!(local
        .read_directory(context.clone(), directory.clone())
        .await
        .is_ok());
    assert!(remote
        .read_directory(context.clone(), directory.clone())
        .await
        .is_err());
    let restarted = fixture.host("Changed hostname");
    assert_eq!(restarted.enroll_worker(None).await.unwrap(), identity);
    assert_eq!(
        fixture.registered(&node_id).await.display_name,
        "User renamed laptop"
    );
    restarted.stop_registration().await.unwrap();
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
    fixture.runtime.shutdown().await.unwrap();
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
    first.stop_registration().await.unwrap();
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
    let notification = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let line = replies.next_line().await.unwrap().unwrap();
            let notification: serde_json::Value = serde_json::from_str(&line).unwrap();
            match notification["method"].as_str() {
                Some("desktop/worker/failed") => break notification,
                Some("desktop/worker/ready") => {}
                other => panic!("Unexpected lifecycle notice: {other:?}"),
            }
        }
    })
    .await
    .unwrap();
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
    restored.stop_registration().await.unwrap();
    assert!(failures.borrow().is_none());
    requests.shutdown().await.unwrap();
    host_task.await.unwrap().unwrap();
    fixture.runtime.shutdown().await.unwrap();
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
    assert!(fixture.home.path().join(".worker-state.json").exists());
    assert!(fixture.home.path().join(".worker-state.lock").exists());
    // Reopen host persistence after the unconfirmed CreateNode response.
    let store = YamlConfigStore::new(fixture.config.source_path());
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
    assert!(!store.node_enrollment_pending().unwrap());
    assert!(!fixture.home.path().join(".worker-state.json").exists());
    assert!(!fixture.home.path().join(".worker-state.lock").exists());
    fixture.registered(&created.node.node_id).await;
    host.stop_registration().await.unwrap();
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
    fixture.runtime.shutdown().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[tokio::test]
async fn desktop_source_files_cross_outgoing_peer_stream_without_session_and_keep_permissions() {
    let fixture = DesktopFixture::new().await;
    fixture.login("user-1");
    let host = fixture.host("Source laptop");
    let node_id = host.enroll_worker(None).await.unwrap().node_id.unwrap();
    fixture.registered(&node_id).await;
    let gateway = agent_client::AgentNodeClient::connect(
        &fixture.config.cloud.agent_base_url,
        agent_client::AgentTlsTrust::Certificate(
            std::fs::read(fixture.config.cloud.agent_trusted_ca_file.as_ref().unwrap()).unwrap(),
        ),
    )
    .await
    .unwrap();
    let peer_id = uuid::Uuid::new_v4().to_string();
    let (peer, _) = gateway
        .connect_coordinator_with_resources(
            "user-1",
            &peer_id,
            &node_id,
            fixture
                .runtime
                .coordinator_resource_handler(node_id.clone()),
        )
        .await
        .unwrap();
    let image = Attachment::new(
        AttachmentKind::Image,
        "source.png",
        "image/png",
        Vec::from(&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..]).into(),
    );
    let owned = fixture
        .runtime
        .tool_image_store
        .save("user-1", vec![image.clone()])
        .await
        .unwrap()
        .remove(0);
    let foreign = fixture
        .runtime
        .tool_image_store
        .save("user-2", vec![image.clone()])
        .await
        .unwrap()
        .remove(0);
    let read = talos_types::FileReadRequest {
        file_id: owned.file_id.clone(),
    };
    let (file, bytes) = gateway
        .read_peer_file("user-1", &peer_id, read.clone(), 1024)
        .await
        .unwrap();
    assert_eq!(file, owned);
    assert_eq!(bytes, image.bytes);
    assert!(gateway
        .read_peer_file("user-1", &peer_id, read.clone(), 1)
        .await
        .unwrap_err()
        .is_code(ErrorCode::AttachmentLimitExceeded));
    assert!(gateway
        .read_peer_file("user-2", &peer_id, read.clone(), 1024)
        .await
        .is_err());
    assert!(gateway
        .read_peer_file(
            "user-1",
            &peer_id,
            talos_types::FileReadRequest {
                file_id: foreign.file_id
            },
            1024
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    let updated = gateway
        .update_peer_file(
            "user-1",
            &peer_id,
            talos_types::FileUpdateRequest {
                file_id: owned.file_id.clone(),
                tags: Some(vec!["shared".into()]),
                metadata: Some(std::collections::BTreeMap::from([(
                    "origin".into(),
                    "source".into(),
                )])),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.file.tags, ["shared"]);
    let cleared = gateway
        .update_peer_file(
            "user-1",
            &peer_id,
            talos_types::FileUpdateRequest {
                file_id: owned.file_id.clone(),
                tags: Some(vec![]),
                metadata: None,
            },
        )
        .await
        .unwrap();
    assert!(cleared.file.tags.is_empty());
    assert_eq!(cleared.file.metadata["origin"], "source");
    assert_eq!(
        fixture
            .runtime
            .service
            .files
            .read(&Actor::regular("user-1"), read.clone())
            .await
            .unwrap()
            .file,
        cleared.file
    );
    let deleted = gateway
        .delete_peer_file(
            "user-1",
            &peer_id,
            talos_types::FileDeleteRequest {
                file_id: owned.file_id.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(deleted.file_id, owned.file_id);
    assert!(fixture
        .runtime
        .service
        .files
        .read(&Actor::regular("user-1"), read.clone())
        .await
        .is_err());
    peer.close();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let error = gateway
                .read_peer_file("user-1", &peer_id, read.clone(), 1024)
                .await
                .unwrap_err();
            if error.is_code(ErrorCode::ServiceUnavailable) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    host.stop_registration().await.unwrap();
    fixture.runtime.shutdown().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[tokio::test]
async fn deleted_desktop_registration_preserves_local_worker_and_explicitly_enrolls_new_uuid() {
    let fixture = DesktopFixture::new().await;
    fixture.login("user-1");
    let host = fixture.host("Disposable laptop");
    let old = host.enroll_worker(None).await.unwrap().node_id.unwrap();
    fixture.registered(&old).await;
    let proof = fixture.home.path().join("keep-workspace.txt");
    std::fs::write(&proof, "retained").unwrap();
    fixture
        .runtime
        .service
        .nodes
        .delete_computer(
            &Actor::regular("user-1").with_access_token("user-1"),
            talos_types::DeleteComputerRequest {
                node_id: old.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        YamlConfigStore::new(fixture.config.source_path())
            .removed_registration_owner()
            .unwrap()
            .as_deref(),
        Some("user-1")
    );
    assert_eq!(
        fixture.runtime.sync_host.local_computer_id().await.unwrap(),
        "embedded"
    );
    assert!(!host.rpc.is_finished());
    let new = host.enroll_worker(None).await.unwrap().node_id.unwrap();
    assert_ne!(new, old);
    fixture.registered(&new).await;
    let deleted = fixture
        .runtime
        .sync_host
        .sync_stores
        .storage
        .query(
            "SELECT owner_user_id FROM computer_deletions WHERE node_id = ?",
            &[&old],
        )
        .await
        .unwrap();
    assert_eq!(
        deleted
            .first()
            .unwrap()
            .get_string("owner_user_id")
            .as_deref(),
        Some("user-1")
    );
    assert_eq!(std::fs::read_to_string(proof).unwrap(), "retained");
    assert_eq!(
        fixture
            .nodes
            .get_node(
                &Actor::regular("user-1"),
                agent_protocol::GetNodeRequest { node_id: old }
            )
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::ResourceExpired
    );
    assert!(!host.rpc.is_finished());
    host.stop_registration().await.unwrap();
    fixture.runtime.shutdown_sync().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn embedded_shutdown_retires_browser_ipc_before_returning() {
    let fixture = DesktopFixture::new().await;
    let browser = fixture.runtime.worker.browser();
    let socket = browser.start_ipc(fixture.home.path()).await.unwrap();
    assert!(tokio::net::UnixStream::connect(&socket).await.is_ok());
    fixture.runtime.shutdown().await.unwrap();
    assert!(browser.shutdown_token().is_cancelled());
    assert!(!socket.exists());
    assert!(!socket.parent().unwrap().exists());
    assert!(tokio::net::UnixStream::connect(&socket).await.is_err());
    fixture.runtime.shutdown_sync().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[tokio::test]
async fn retained_metadata_and_identity_do_not_wait_for_registration_retirement() {
    use talos_app_protocol::{
        requests::{ClientRequest, ClientRequestParams},
        ClientMethod,
    };
    use talos_app_server::AppServerApplication;
    let fixture = DesktopFixture::new().await;
    fixture.login("user-1");
    let host = fixture.host("Local metadata");
    let registered = host.enroll_worker(None).await.unwrap().node_id.unwrap();
    let retirement = fixture
        .runtime
        .sync_host
        .registration_retirement
        .lock()
        .await;
    YamlConfigStore::new(fixture.config.source_path())
        .mark_registration_removed(&registered, "user-1")
        .unwrap();
    let actor = Actor::regular("user-1").with_access_token("user-1");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        let identity = host
            .dispatch(
                &actor,
                ClientRequest {
                    method: ClientMethod::DesktopWorkerIdentity,
                    params: ClientRequestParams::DesktopWorkerIdentity(Default::default()),
                },
            )
            .await
            .unwrap();
        assert_eq!(identity.body["nodeId"], "embedded");
        assert_eq!(
            fixture
                .runtime
                .sync_host
                .local_computer_for_client(&actor)
                .await
                .unwrap(),
            Some("embedded".into())
        );
        fixture
            .runtime
            .service
            .list_sessions(
                &actor,
                talos_types::SessionListRequest {
                    node_id: Some("embedded".into()),
                    limit: None,
                    offset: None,
                    project_id: None,
                },
            )
            .await
            .unwrap();
        fixture
            .runtime
            .service
            .list_projects(
                &actor,
                talos_types::ProjectListRequest {
                    node_id: Some("embedded".into()),
                },
            )
            .await
            .unwrap();
        fixture.runtime.service.sync_status(&actor).await.unwrap();
    })
    .await
    .expect("retained reads must not await registration cleanup");
    drop(retirement);
    host.stop_registration().await.unwrap();
    fixture.runtime.shutdown().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}
