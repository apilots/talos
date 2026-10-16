//! Real management runtimes, G talos-server, WSS/Loro H and one physical Worker.
//! The model HTTP fixture controls timing; paid-model deployment E2E is stage 5.
use super::*;
use axum::{extract::State, response::IntoResponse, routing::post, Json, Router};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};
use talos_auth::RequestIdentity;
use talos_config::{AgentNodeRunnerConfig, AgentNodeTransport, Config, TalosPaths};
use talos_store::session::SessionQuery;
use talos_sync::SyncConnectionState;
use talos_types::{AgentUserContent, AgentUserInput, StartTurnRequest};

#[path = "application_sync_login_tests.rs"]
mod login;
#[path = "application_sync_metrics_tests.rs"]
mod metrics;

struct HubProcess(std::process::Child);
impl HubProcess {
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for HubProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

const ACCOUNT: &str = "same-account";
const TOKEN: &str = "shared-session-fixture-token";
const OTHER_TOKEN: &str = "other-account-fixture-token";
#[derive(Clone, Default)]
struct Model {
    blocked: Arc<std::sync::atomic::AtomicBool>,
    resume: Arc<tokio::sync::Notify>,
    prompts: Arc<std::sync::Mutex<Vec<Value>>>,
}
async fn introspect(Json(request): Json<Value>) -> Json<Value> {
    Json(
        json!({"active":request["token"] == TOKEN || request["token"] == OTHER_TOKEN,
        "user":{"user_id":if request["token"] == OTHER_TOKEN { "other-account" } else { ACCOUNT },"email":"test@example.invalid","username":"test","role":"user","type":"free","status":"active"},
        "capabilities":{"is_admin":false,"role":"user","scopes":["sessions:read","sessions:write","projects:read","projects:write"]},
        "entitlement":{"can_use":true,"plan":"free","reason":null},"expires_at_ms":null}),
    )
}
async fn completion(
    State(model): State<Model>,
    Json(request): Json<Value>,
) -> axum::response::Response {
    model.prompts.lock().unwrap().push(request.clone());
    if request["stream"] == true {
        if model.blocked.load(std::sync::atomic::Ordering::SeqCst) {
            model.resume.notified().await;
        }
        let chunk = json!({"id":"fixture","object":"chat.completion.chunk","created":1,"model":"deepseek-chat","choices":[{"index":0,"delta":{"role":"assistant","content":"worker response"},"finish_reason":null}]});
        let end = json!({"id":"fixture","object":"chat.completion.chunk","created":1,"model":"deepseek-chat","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
        (
            [("content-type", "text/event-stream")],
            format!("data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n"),
        )
            .into_response()
    } else {
        Json(json!({"id":"fixture","object":"chat.completion","created":1,"model":"deepseek-chat","choices":[{"index":0,"message":{"role":"assistant","content":"worker response"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}})).into_response()
    }
}
fn port() -> std::net::SocketAddr {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    socket.local_addr().unwrap()
}
fn persisted_config(config: &Config) -> Vec<u8> {
    let mut value = serde_json::to_value(config).unwrap();
    value["logging"].as_object_mut().unwrap().remove("file");
    value["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    serde_json::to_vec_pretty(&value).unwrap()
}
fn config(home: &Path, api: &str) -> Config {
    let paths = TalosPaths::from_home(home.into());
    paths.ensure_base_dirs().unwrap();
    let mut config = Config::default().with_runtime_paths(paths);
    config.apply_derived_paths();
    config.cloud.api_base_url = api.into();
    config.gateway.host = "127.0.0.1".into();
    config.gateway.port = port().port();
    config.gateway.auth = true;
    config.llm.provider = "deepseek".into();
    config.llm.model = "deepseek-chat".into();
    config.llm.deepseek.base_url = Some(api.into());
    config.llm.deepseek.api_key = Some("controlled-fixture-key".into());
    config.llm.deepseek.models = vec![talos_config::ProviderModelConfig {
        id: "deepseek-chat".into(),
        name: "Controlled timing".into(),
        context_window: 65536,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: vec![],
        cost: Default::default(),
    }];
    config.sync.enabled = Some(true);
    // Runtime upgrades still read the original host config for resource policies.
    std::fs::write(config.source_path(), persisted_config(&config)).unwrap();
    config
}
fn start_request(session: &str, text: &str) -> StartTurnRequest {
    StartTurnRequest {
        node_id: None,
        session_id: session.into(),
        input: AgentUserInput {
            content: vec![AgentUserContent::Text { text: text.into() }],
        },
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    }
}
async fn wait_for(label: &str, mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Product flow did not converge: {label}"));
}
async fn worker_state(
    runtime: &TalosApplicationRuntime,
    session: &str,
) -> agent_protocol::SessionExecutionState {
    let record = runtime
        .session_store
        .get_session_record(session)
        .await
        .unwrap()
        .unwrap();
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id,
        session_id: record.id,
        workspace: runtime
            .config
            .resolve_workspace(&record.workspace_id, ACCOUNT)
            .unwrap(),
    };
    talos_coordinator::AgentCoordinator::read_session_execution_state(
        &*runtime.coordinator,
        context,
    )
    .await
    .unwrap()
    .unwrap()
}
async fn close(runtime: TalosApplicationRuntime) {
    runtime.shutdown_sync().await.unwrap();
    if let Some(worker) = runtime.worker_registration.lock().await.take() {
        worker.shutdown().await.unwrap();
    }
    runtime.worker_rpc.shutdown().await.unwrap();
    runtime.resource_rpc.shutdown().await.unwrap();
    runtime.automation.shutdown().await.unwrap();
    runtime.worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn shared_session_continues_on_the_same_worker_and_active_commands_return_to_its_coordinator()
{
    let _logging = talos_logging::init().unwrap();
    let directory = tempfile::Builder::new()
        .prefix(".talos-sync-flow-")
        .tempdir_in(std::env::var_os("HOME").unwrap())
        .unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let cert = directory.path().join("cert.pem");
    let key = directory.path().join("key.pem");
    std::fs::write(&cert, certificate.cert.pem()).unwrap();
    std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
    let model = Model::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/auth/introspect", post(introspect))
        .route("/chat/completions", post(completion))
        .with_state(model.clone());
    let http = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let hub_address = port();
    let hub_config = directory.path().join("sync.yaml");
    std::fs::write(&hub_config, serde_json::to_vec_pretty(&json!({"listener":{"bind_address":hub_address.to_string(),"certificate_file":cert,"private_key_file":key},"storage":{"backend":"sqlite","path":directory.path().join("hub.db"),"max_connections":1,"busy_timeout_ms":5000},"auth":{"issuer_url":api,"timeout_ms":5000,"revalidate_interval_ms":1000}})).unwrap()).unwrap();
    let binary = std::env::var_os("TALOS_SYNC_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../../talos-sync/target/debug/talos-sync")
        });
    let log = std::fs::File::create(directory.path().join("sync.log")).unwrap();
    let mut hub = HubProcess(
        std::process::Command::new(binary)
            .arg("--config")
            .arg(hub_config)
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("Build the independent Sync binary before this native E2E"),
    );
    wait_for("H listener", async || {
        tokio::net::TcpStream::connect(hub_address).await.is_ok()
    })
    .await;
    let mut options = talos_sync_sdk::SyncOptions::new(
        format!("wss://{hub_address}/v1/sync"),
        ACCOUNT.into(),
        uuid::Uuid::new_v4().to_string(),
    );
    options.trusted_ca_pem = Some(std::fs::read(&cert).unwrap());
    let credential: talos_sync_sdk::CredentialCallback = Arc::new(|| {
        Box::pin(async {
            Ok(talos_sync_sdk::VerifiedCredential {
                account_id: ACCOUNT.into(),
                access_token: TOKEN.into(),
            })
        })
    });
    let discovery = talos_sync_sdk::SyncClient::connect(options, credential.clone())
        .await
        .unwrap_or_else(|error| {
            panic!(
                "Sync handshake failed: {error:?}; {}",
                std::fs::read_to_string(directory.path().join("sync.log")).unwrap()
            )
        });
    let scope = discovery.scope().await;
    assert!(!scope.hub_id.is_empty());
    discovery.shutdown().await.unwrap();
    let mut gateway_config = config(&directory.path().join("gateway"), &api);
    let rpc = port();
    gateway_config.agent_node.control_plane = Some(talos_config::AgentNodeControlConfig {
        bind_address: rpc.to_string(),
        tls_certificate_file: cert.clone(),
        tls_private_key_file: key.clone(),
        trusted_node_ca_file: Some(cert.clone()),
    });
    gateway_config.cloud.agent_base_url = format!("https://{rpc}");
    gateway_config.cloud.agent_trusted_ca_file = Some(cert.clone());
    gateway_config.sync.hub_endpoint = format!("wss://{hub_address}/v1/sync");
    gateway_config.sync.trusted_ca_file = Some(cert.clone());
    std::fs::write(
        gateway_config.source_path(),
        persisted_config(&gateway_config),
    )
    .unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let report = startup::FinishStartupReport {
        resources: talos_upgrade::RuntimeResourcesReport {
            database_migrations: talos_config::DatabaseMigrationsInstallReport {
                source_dir: migration.parent().unwrap().into(),
                target_dir: gateway_config.talos_paths().conf_dir.join("migrations"),
                backend_dir: migration.clone(),
                database_type: talos_config::DatabaseType::Sqlite,
                changed: false,
            },
        },
    };
    let gateway_http_port = gateway_config.gateway.port;
    let server = start_server_after_startup(gateway_config, None, &report)
        .await
        .unwrap();
    let server_handle = server.handle();
    let gateway_task = tokio::spawn(server.run());
    wait_for("G listener", async || {
        tokio::net::TcpStream::connect(rpc).await.is_ok()
    })
    .await;
    let endpoint = format!("https://{rpc}");
    let gateway = agent_client::AgentNodeClient::connect(
        &endpoint,
        agent_client::AgentTlsTrust::Certificate(std::fs::read(&cert).unwrap()),
    )
    .await
    .unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let mut peers = Vec::new();
    for name in ["desktop-a", "desktop-b"] {
        let node = gateway
            .create_node(
                TOKEN,
                agent_protocol::CreateNodeRequest {
                    display_name: name.into(),
                    endpoint: None,
                    owner_user_id: None,
                    credential_expires_at: chrono::Utc::now().timestamp() + 3600,
                },
            )
            .await
            .unwrap();
        let mut peer = config(&directory.path().join(name), &api);
        let credential_file = peer.talos_paths().conf_dir.join("worker.credential");
        talos_fs::write_bytes_with(
            &credential_file,
            node.credential.token.as_bytes(),
            talos_fs::WriteOptions::default()
                .with_permissions(talos_fs::FilePermissions::unix(0o600)),
        )
        .unwrap();
        peer.agent_node.runner = Some(AgentNodeRunnerConfig {
            node_id: node.node.node_id,
            transport: Some(AgentNodeTransport::Outbound),
            control_plane_endpoint: endpoint.clone(),
            credential_file,
            trusted_ca_file: Some(cert.clone()),
        });
        peer.cloud.agent_base_url = endpoint.clone();
        peer.cloud.agent_trusted_ca_file = Some(cert.clone());
        peer.sync.hub_endpoint = format!("wss://{}/v1/sync", hub_address);
        peer.sync.trusted_ca_file = Some(cert.clone());
        std::fs::write(peer.source_path(), persisted_config(&peer)).unwrap();
        peers.push(
            build_application_runtime_with_migration_dir(peer, &migration)
                .await
                .unwrap(),
        );
    }
    let b = peers.pop().unwrap();
    let a = peers.pop().unwrap();
    let computer = a.config.agent_node.runner.as_ref().unwrap().node_id.clone();
    for peer in [&a, &b] {
        let node = &peer.config.agent_node.runner.as_ref().unwrap().node_id;
        wait_for("Worker registered on G", async || {
            gateway
                .get_node(TOKEN, node)
                .await
                .is_ok_and(|node| node.available)
        })
        .await;
    }
    let actor = talos_service::Actor::regular(ACCOUNT).with_access_token(TOKEN);
    let identity = RequestIdentity {
        user_id: ACCOUNT.into(),
        username: None,
        role: talos_auth::UserRole::User,
        auth_disabled: false,
        access_token: Some(TOKEN.into()),
    };
    // A registered remote Computer never grants access to G's private local
    // Worker; both explicit resources and execution must enforce the host policy.
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response: serde_json::Value = client
        .get(format!(
            "http://127.0.0.1:{gateway_http_port}/v1/models?nodeId=embedded"
        ))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], "permissionDenied");
    let mut private_request = start_request("private-server-session", "must not execute");
    private_request.node_id = Some("embedded".into());
    let response: serde_json::Value = client
        .post(format!(
            "http://127.0.0.1:{gateway_http_port}/v1/turns/start"
        ))
        .bearer_auth(TOKEN)
        .json(&private_request)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], "permissionDenied");
    // A newly signed-in host enrolls its existing Worker, persists its UUID, and resumes with an explicit opt-out.
    let mut late_config = config(&directory.path().join("desktop-late-enrollment"), &api);
    late_config.cloud.agent_base_url = endpoint.clone();
    late_config.cloud.agent_trusted_ca_file = Some(cert.clone());
    late_config.sync.hub_endpoint = format!("wss://{hub_address}/v1/sync");
    late_config.sync.trusted_ca_file = Some(cert.clone());
    std::fs::write(late_config.source_path(), persisted_config(&late_config)).unwrap();
    metrics::verify_late_enrollment(late_config, &migration, &actor, &scope).await;
    let mut fresh_config = config(&directory.path().join("desktop-login"), &api);
    fresh_config.cloud.agent_base_url = endpoint.clone();
    fresh_config.cloud.agent_trusted_ca_file = Some(cert.clone());
    fresh_config.sync.hub_endpoint = format!("wss://{hub_address}/v1/sync");
    fresh_config.sync.trusted_ca_file = Some(cert.clone());
    fresh_config.sync.enabled = None;
    std::fs::write(fresh_config.source_path(), persisted_config(&fresh_config)).unwrap();
    let fresh = login::verify(fresh_config, &migration, &actor, &gateway).await;

    // A saved local UUID cannot bind another verified account to this physical Worker.
    let other = talos_auth::RequestIdentity {
        user_id: "other-account".into(),
        access_token: Some(OTHER_TOKEN.into()),
        ..identity.clone()
    };
    let denied = a
        .start_account_sync(
            &other,
            talos_sync::SyncBindParams {
                scope: talos_sync_types::SyncScope {
                    account_id: other.user_id.clone(),
                    ..scope.clone()
                },
                source_user_id: other.user_id.clone(),
                computers: vec![talos_sync::ComputerAlias {
                    source_node_id: "embedded".into(),
                    computer_id: computer.clone(),
                }],
            },
            credential.clone(),
        )
        .await;
    assert!(denied.is_err());
    assert!(a.sync.account_binding(&other).await.unwrap().is_none());

    let mut handles = Vec::new();
    for peer in [&a, &b] {
        // The caller proves ownership of the configured UUID through G before
        // an unregistered public Server alias may be treated as its local Computer.
        peer.service
            .workspace_info(
                &actor,
                talos_types::WorkspaceInfoRequest {
                    node_id: Some(
                        peer.config
                            .agent_node
                            .runner
                            .as_ref()
                            .unwrap()
                            .node_id
                            .clone(),
                    ),
                },
            )
            .await
            .unwrap();
        let status = peer.service.sync_status(&actor).await.unwrap();
        assert_eq!(
            status.local_computer_id.as_deref(),
            Some(
                peer.config
                    .agent_node
                    .runner
                    .as_ref()
                    .unwrap()
                    .node_id
                    .as_str()
            )
        );
        handles.push(
            peer.sync_stores
                .handles
                .lock()
                .await
                .get(ACCOUNT)
                .unwrap()
                .clone(),
        );
    }

    wait_for("A and B WSS connected", async || {
        handles
            .iter()
            .all(|handle| handle.status().connection == SyncConnectionState::Connected)
    })
    .await;
    metrics::verify(&a, &b, &actor, gateway_http_port).await;
    let local_llm = a
        .service
        .get_config(
            &actor,
            talos_types::ConfigGetRequest {
                node_id: None,
                path: talos_types::ConfigPath::Llm,
            },
        )
        .await
        .unwrap();
    assert!(matches!(local_llm, talos_types::ConfigResponse::Llm { .. }));
    let local_search = a
        .service
        .get_config(
            &actor,
            talos_types::ConfigGetRequest {
                node_id: None,
                path: talos_types::ConfigPath::WebSearch,
            },
        )
        .await
        .unwrap();
    let remote_search = b
        .service
        .get_config(
            &actor,
            talos_types::ConfigGetRequest {
                node_id: Some(computer.clone()),
                path: talos_types::ConfigPath::WebSearch,
            },
        )
        .await
        .unwrap();
    assert_eq!(local_search, remote_search);
    let original_command = talos_types::StartTurnRequest {
        node_id: Some(computer.clone()),
        client_request_id: Some("shared-command-replay".into()),
        ..start_request("shared-session", "first on A")
    };
    let original_response = a
        .service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            original_command.clone(),
            Some("shared-command-replay".into()),
        )
        .await
        .unwrap();
    wait_for("A first Turn settled", async || {
        a.service
            .get_active_turn(&actor, "shared-session")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    wait_for("A facts imported on B", async || {
        b.session_store
            .get_session_record("shared-session")
            .await
            .unwrap()
            .is_some()
    })
    .await;
    wait_for("A history_cursor Shared", async || {
        worker_state(&a, "shared-session").await.handoff_ready
    })
    .await;
    b.connect_computer(&identity, &computer).await.unwrap();
    assert_eq!(
        b.service
            .start_turn(
                &actor,
                talos_types::talos::SessionSource::http(),
                original_command,
                Some("shared-command-replay".into())
            )
            .await
            .unwrap(),
        original_response
    );
    let info = b
        .service
        .get_session(&actor, "shared-session")
        .await
        .unwrap();
    assert_eq!(info.node_id, computer);
    b.service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "continue on B"),
            None,
        )
        .await
        .unwrap();
    wait_for("B continued Turn settled", async || {
        b.service
            .get_active_turn(&actor, "shared-session")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    wait_for("B history_cursor Shared", async || {
        worker_state(&a, "shared-session").await.handoff_ready
    })
    .await;
    let state = worker_state(&a, "shared-session").await;
    assert_eq!(state.coordinator_id.as_deref(), Some(b.sync_peer_id()));
    assert!(model
        .prompts
        .lock()
        .unwrap()
        .iter()
        .any(|prompt| prompt.to_string().contains("first on A")
            && prompt.to_string().contains("continue on B")));
    assert_eq!(
        a.session_store
            .list_session_records(&SessionQuery {
                execution_node_id: Some("embedded".into()),
                ..Default::default()
            })
            .await
            .unwrap()
            .len(),
        1
    );
    // Both coordinators contend while the real Kernel's upstream HTTP is held.
    model
        .blocked
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (left, right) = tokio::join!(
        a.service.start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "contend A"),
            None
        ),
        b.service.start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "contend B"),
            None
        )
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let (current, remote, accepted) = match (left, right) {
        (Ok(turn), Err(_)) => (&a, &b, turn),
        (Err(_), Ok(turn)) => (&b, &a, turn),
        _ => unreachable!(),
    };
    wait_for(
        "live Turn visible on the other management database",
        async || {
            remote
                .service
                .get_active_turn(&actor, "shared-session")
                .await
                .is_ok_and(|state| {
                    state
                        .active_turn
                        .is_some_and(|active| active.identity.turn_id == accepted.turn_id)
                })
        },
    )
    .await;
    let input_identity = talos_types::AgentTurnRef::new("shared-session", &accepted.turn_id);
    let input = remote
        .service
        .submit_turn_input(
            &actor,
            talos_agent_api::SubmitTurnInputRequest {
                identity: input_identity.clone(),
                input: talos_agent_api::SubmitTurnInput::Message {
                    input: AgentUserInput {
                        content: vec![AgentUserContent::Text {
                            text: "live input through G".into(),
                        }],
                    },
                },
                client_request_id: Some("live-input-request".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(input.identity, input_identity);
    let queued = remote
        .service
        .add_queued_message(
            &actor,
            talos_types::MessageQueueAddRequest {
                session_id: "shared-session".into(),
                input: AgentUserInput {
                    content: vec![AgentUserContent::Text {
                        text: "queued through G".into(),
                    }],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        current
            .service
            .list_message_queue(
                &actor,
                talos_types::MessageQueueListRequest {
                    session_id: "shared-session".into()
                }
            )
            .await
            .unwrap(),
        queued
    );
    remote
        .service
        .delete_queued_message(
            &actor,
            talos_types::MessageQueueDeleteRequest {
                session_id: "shared-session".into(),
                message_id: queued.messages[0].id.clone(),
            },
        )
        .await
        .unwrap();
    remote
        .service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new("shared-session", accepted.turn_id),
            },
        )
        .await
        .unwrap();
    model
        .blocked
        .store(false, std::sync::atomic::Ordering::SeqCst);
    model.resume.notify_waiters();
    wait_for("cancelled Turn Shared", async || {
        worker_state(&a, "shared-session").await.handoff_ready
    })
    .await;
    // Source-owned attachment bodies stay outside H and are read through G.
    let file = a
        .service
        .files
        .upload(
            &actor,
            talos_types::FileUploadRequest {
                filename: "shared.png".into(),
                scope: talos_types::FileScope::Personal,
                tags: vec![],
                metadata: Default::default(),
            },
            Box::pin(tokio_stream::once(Ok(bytes::Bytes::from_static(
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR",
            )))),
        )
        .await
        .unwrap()
        .file;
    let file_request = talos_types::FileReadRequest {
        file_id: file.file_id.clone(),
    };
    wait_for("attachment metadata imported", async || {
        b.service
            .files
            .read(&actor, file_request.clone())
            .await
            .is_ok()
    })
    .await;
    assert_eq!(
        b.service
            .files
            .read_content(&actor, file_request.clone())
            .await
            .unwrap()
            .bytes,
        bytes::Bytes::from_static(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
    );
    b.service
        .files
        .update(
            &actor,
            talos_types::FileUpdateRequest {
                file_id: file.file_id.clone(),
                tags: Some(vec!["remote-update".into()]),
                metadata: None,
            },
        )
        .await
        .unwrap();
    wait_for("source File update synchronized", async || {
        b.service
            .files
            .read(&actor, file_request.clone())
            .await
            .is_ok_and(|reply| reply.file.tags == vec!["remote-update"])
    })
    .await;
    b.service
        .files
        .delete(
            &actor,
            talos_types::FileDeleteRequest {
                file_id: file.file_id.clone(),
            },
        )
        .await
        .unwrap();
    wait_for("source File delete synchronized", async || {
        b.service
            .files
            .read(&actor, file_request.clone())
            .await
            .is_err()
    })
    .await;
    // Clear advances the original generation; close must not prevent handoff.
    b.service
        .clear_session(&actor, "shared-session")
        .await
        .unwrap();
    wait_for("clear generation Shared", async || {
        let state = worker_state(&a, "shared-session").await;
        state.handoff_ready && state.history_cursor.history_generation == 1
    })
    .await;
    a.service
        .close_session(&actor, "shared-session")
        .await
        .unwrap();
    wait_for("closed Session Shared", async || {
        worker_state(&a, "shared-session").await.handoff_ready
    })
    .await;
    a.service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("delete-session", "delete after sharing"),
            None,
        )
        .await
        .unwrap();
    wait_for("deletable Session Shared", async || {
        worker_state(&a, "delete-session").await.handoff_ready
    })
    .await;
    wait_for("deletable Session imported", async || {
        b.session_store
            .get_session_record("delete-session")
            .await
            .unwrap()
            .is_some()
    })
    .await;
    assert!(b
        .service
        .delete_session(&actor, "delete-session")
        .await
        .unwrap());
    wait_for(
        "Session deletion returned to original management database",
        async || {
            a.session_store
                .get_session_record("delete-session")
                .await
                .unwrap()
                .is_none()
        },
    )
    .await;
    // Explicit opt-out preserves local capture and never disables the Worker's private RPC.
    let disabled = a.service.disable_sync(&actor).await.unwrap();
    assert_eq!(
        disabled.connection,
        talos_types::SyncConnectionState::Disabled
    );
    a.service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("paused-sharing", "continue while sharing is disabled"),
            None,
        )
        .await
        .unwrap();
    wait_for("paused local Turn settled", async || {
        a.service
            .get_active_turn(&actor, "paused-sharing")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    let paused = a.service.sync_status(&actor).await.unwrap();
    assert_eq!(
        paused.connection,
        talos_types::SyncConnectionState::Disabled
    );
    assert!(paused.pending_changes > 0);
    assert!(b
        .session_store
        .get_session_record("paused-sharing")
        .await
        .unwrap()
        .is_none());
    a.service
        .enable_sync(&actor, talos_types::SyncEnableRequest::default())
        .await
        .unwrap();
    handles[0] = a
        .sync_stores
        .handles
        .lock()
        .await
        .get(ACCOUNT)
        .unwrap()
        .clone();
    wait_for("disabled-period facts shared after enabling", async || {
        b.session_store
            .get_session_record("paused-sharing")
            .await
            .unwrap()
            .is_some()
    })
    .await;

    close(fresh).await;
    // A cached Shared history_cursor permits a new local Turn with H stopped.
    wait_for("A and B terminal history_cursor equal", async || {
        a.session_store
            .get_history_cursor("shared-session")
            .await
            .unwrap()
            == b.session_store
                .get_history_cursor("shared-session")
                .await
                .unwrap()
    })
    .await;
    hub.stop();
    a.service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "local without sync"),
            None,
        )
        .await
        .unwrap();
    wait_for("offline local Turn settled", async || {
        a.service
            .get_active_turn(&actor, "shared-session")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    assert!(!worker_state(&a, "shared-session").await.handoff_ready);
    assert!(b
        .service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "stale B history"),
            None
        )
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Conflict));
    let restart_config = a.config.clone();
    close(a).await;
    let a = build_application_runtime_with_migration_dir(restart_config, &migration)
        .await
        .unwrap();
    a.service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "local after Worker restart"),
            None,
        )
        .await
        .unwrap();
    wait_for("restarted local Turn settled", async || {
        a.service
            .get_active_turn(&actor, "shared-session")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    assert_eq!(
        worker_state(&a, "shared-session")
            .await
            .history_cursor
            .history_generation,
        1
    );
    assert!(!worker_state(&a, "shared-session").await.handoff_ready);
    assert!(model.prompts.lock().unwrap().iter().any(|prompt| {
        let prompt = prompt.to_string();
        prompt.contains("local after Worker restart")
            && prompt.contains("local without sync")
            && !prompt.contains("first on A")
    }));
    // Reproduce a host exit after the original Store reset commits, before its
    // Worker history_cursor commit. No fault flag or alternate production path is used.
    let record = a
        .session_store
        .get_session_record("shared-session")
        .await
        .unwrap()
        .unwrap();
    let old_history_cursor = worker_state(&a, "shared-session").await.history_cursor;
    a.coordinator
        .clear_session(
            agent_protocol::AgentSessionContext {
                agent_id: record.agent_id.clone(),
                session_id: record.id.clone(),
                workspace: a
                    .config
                    .resolve_workspace(&record.workspace_id, ACCOUNT)
                    .unwrap(),
            },
            old_history_cursor.clone(),
        )
        .await
        .unwrap();
    assert!(a
        .session_store
        .clear_session_history(&record.id, 0)
        .await
        .unwrap());
    assert_eq!(
        worker_state(&a, "shared-session").await.history_cursor,
        old_history_cursor
    );
    let restart_config = a.config.clone();
    close(a).await;
    let a = build_application_runtime_with_migration_dir(restart_config, &migration)
        .await
        .unwrap();
    a.service
        .start_turn(
            &actor,
            talos_types::talos::SessionSource::http(),
            start_request("shared-session", "continue after interrupted reset commit"),
            None,
        )
        .await
        .unwrap();
    wait_for("interrupted reset recovered without H", async || {
        a.service
            .get_active_turn(&actor, "shared-session")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    assert_eq!(
        worker_state(&a, "shared-session")
            .await
            .history_cursor
            .history_generation,
        2
    );
    close(b).await;
    close(a).await;
    server_handle.shutdown().await.unwrap();
    gateway_task.await.unwrap().unwrap();
    http.abort();
}
