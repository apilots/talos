//! Central administrator claim uses gRPC Connect and preserves personal ownership.
use super::*;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::path::Path;
use talos_config::{AgentNodeTransport, Config, TalosPaths};
use talos_coordinator::AgentCoordinator;

const TOKEN: &str = "regular-user";
const ADMIN_TOKEN: &str = "first-admin";
const OTHER_ADMIN_TOKEN: &str = "other-admin";
const OWNER_TOKEN: &str = "owner-without-admin-role";

async fn introspect(Json(request): Json<Value>) -> Json<Value> {
    let token = request["token"].as_str().unwrap_or_default();
    let (account, role) = match token {
        ADMIN_TOKEN => ("server-admin", "admin"),
        OWNER_TOKEN => ("server-admin", "user"),
        OTHER_ADMIN_TOKEN => ("other-admin", "admin"),
        TOKEN => ("regular-user", "user"),
        _ => ("", "user"),
    };
    Json(json!({"active": !account.is_empty(),
        "user":{"user_id":account,"email":"test@example.invalid","username":"test","role":role,"type":"free","status":"active"},
        "capabilities":{"is_admin":role == "admin","role":role,"scopes":[]},
        "entitlement":{"can_use":true,"plan":"free","reason":null},"expires_at_ms":null}))
}

async fn wait_for(label: &str, mut condition: impl AsyncFnMut() -> bool) {
    // Registration can retry after five seconds; availability then requires
    // the first ten-second heartbeat. Include both without a fixed sleep.
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while !condition().await {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("condition did not converge: {label}"));
}

fn port() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

#[tokio::test]
async fn administrator_claims_direct_server_worker_without_reassigning_it() {
    let directory = tempfile::Builder::new()
        .prefix(".talos-server-owner-")
        .tempdir_in(std::env::var_os("HOME").unwrap())
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route("/v1/auth/introspect", post(introspect));
    let auth_server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let paths = TalosPaths::from_home(directory.path().to_path_buf());
    paths.ensure_base_dirs().unwrap();
    let mut config = Config::default().with_runtime_paths(paths);
    config.apply_derived_paths();
    config.gateway.auth = true;
    config.gateway.host = "127.0.0.1".into();
    config.gateway.port = port().port();
    config.cloud.api_base_url = api;
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
        id: config.llm.model.clone(),
        name: "Controlled fixture".into(),
        context_window: 32768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: vec![],
        cost: Default::default(),
    }];
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let cert = directory.path().join("server.crt");
    let key = directory.path().join("server.key");
    std::fs::write(&cert, certificate.cert.pem()).unwrap();
    std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
    let rpc = port();
    config.agent_node.control_plane = Some(talos_config::AgentNodeControlConfig {
        bind_address: rpc.to_string(),
        tls_certificate_file: cert.clone(),
        tls_private_key_file: key,
    });
    config.cloud.agent_base_url = format!("https://{rpc}");
    config.cloud.agent_trusted_ca_file = Some(cert.clone());
    config.sync.enabled = Some(false);
    let mut document = serde_json::to_value(&config).unwrap();
    document["logging"].as_object_mut().unwrap().remove("file");
    document["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    std::fs::write(config.source_path(), serde_json::to_vec(&document).unwrap()).unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let report = startup::FinishStartupReport {
        resources: talos_upgrade::RuntimeResourcesReport {
            database_migrations: talos_config::DatabaseMigrationsInstallReport {
                source_dir: migration.clone(),
                target_dir: migration.clone(),
                backend_dir: migration,
                database_type: talos_config::DatabaseType::Sqlite,
                changed: false,
            },
        },
    };
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://127.0.0.1:{}", config.gateway.port);
    let mut expected_id: Option<String> = None;
    let mut expected_credential = None;
    for iteration in 0..3 {
        let server = start_server_after_startup(config.reload().unwrap(), None, &report)
            .await
            .unwrap();
        let handle = server.handle();
        let coordinator = server.coordinator.clone();
        let host = server.host.clone();
        let nodes = server.host.service.nodes.clone();
        let worker_failures = server.host.worker_failures.subscribe();
        let task = tokio::spawn(server.run());
        wait_for("central RPC ready", async || {
            tokio::net::TcpStream::connect(rpc).await.is_ok()
        })
        .await;
        let gateway = agent_client::AgentNodeClient::connect(
            &config.cloud.agent_base_url,
            agent_client::AgentTlsTrust::Certificate(std::fs::read(&cert).unwrap()),
        )
        .await
        .unwrap();
        if iteration == 2 {
            // A saved direct registration must recover without a login/status
            // request triggering enrollment again after the Server restarts.
            let id = expected_id.as_ref().unwrap();
            wait_for("saved direct Worker available before login", async || {
                assert!(
                    worker_failures.borrow().is_none(),
                    "startup registration failed: {:?}",
                    *worker_failures.borrow()
                );
                nodes
                    .get_node(
                        &talos_service::Actor::regular("server-admin"),
                        talos_types::GetNodeRequest {
                            node_id: id.clone(),
                        },
                    )
                    .await
                    .is_ok_and(|node| node.available)
            })
            .await;
            assert!(!directory.path().join(".worker-state.json").exists());
            assert!(!directory.path().join(".worker-state.lock").exists());
        }
        if iteration == 0 {
            let status: Value = client
                .get(format!("{url}/v1/sync/status"))
                .bearer_auth(TOKEN)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(status["data"]["localComputerId"].is_null());
            assert!(config.reload().unwrap().agent_node.runner.is_none());
        }
        let owner_token = if iteration == 2 {
            OWNER_TOKEN
        } else {
            ADMIN_TOKEN
        };
        host.schedule_refresh(
            &talos_service::Actor::regular("server-admin")
                .with_role(if owner_token == ADMIN_TOKEN {
                    talos_auth::UserRole::Admin
                } else {
                    talos_auth::UserRole::User
                })
                .with_access_token(owner_token),
        );
        let mut status = Value::Null;
        wait_for("background Server enrollment", async || {
            status = client
                .get(format!("{url}/v1/sync/status"))
                .bearer_auth(owner_token)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            status["data"]["localComputerId"].is_string()
        })
        .await;
        let id = status["data"]["localComputerId"]
            .as_str()
            .expect("claiming account keeps its personal Server Worker")
            .to_owned();
        if let Some(expected) = &expected_id {
            assert_eq!(&id, expected);
        }
        wait_for("direct Server Worker available", async || {
            gateway
                .get_node(ADMIN_TOKEN, &id)
                .await
                .is_ok_and(|node| node.available)
        })
        .await;
        let node = gateway.get_node(ADMIN_TOKEN, &id).await.unwrap();
        assert_eq!(node.owner_user_id.as_deref(), Some("server-admin"));
        assert_eq!(
            node.endpoint.as_deref(),
            Some(config.cloud.agent_base_url.as_str())
        );
        let saved = config.reload().unwrap();
        let runner = saved.agent_node.runner.unwrap();
        assert_eq!(runner.transport(), AgentNodeTransport::Direct);
        let credential = std::fs::read(&runner.credential_file).unwrap();
        if let Some(expected) = &expected_credential {
            assert_eq!(&credential, expected);
        }
        let own_models: Value = client
            .get(format!("{url}/v1/models?nodeId=embedded"))
            .bearer_auth(owner_token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(own_models["status"], "success");
        for token in [TOKEN, OTHER_ADMIN_TOKEN] {
            // Restore identity explicitly before exercising local data authorization.
            host.auth_service.verify_access_token(token).await.unwrap();
            let status: Value = client
                .get(format!("{url}/v1/sync/status"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(status["data"]["localComputerId"].is_null());
            let reply: Value = client
                .get(format!("{url}/v1/models?nodeId=embedded"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(reply["error"]["code"], "permissionDenied");
        }
        assert_eq!(std::fs::read(&runner.credential_file).unwrap(), credential);
        assert!(worker_failures.borrow().is_none());
        assert!(gateway.get_node(ADMIN_TOKEN, &id).await.unwrap().available);
        if iteration == 0 {
            // Another administrator may have passed the empty-identity check
            // before the owner finished claiming. Reject that delayed enrollment
            // without stopping the already registered owner's Worker.
            let other = host
                .auth_service
                .verify_access_token(OTHER_ADMIN_TOKEN)
                .await
                .unwrap();
            let store = talos_config::YamlConfigStore::new(config.source_path());
            let result = super::desktop_worker::WorkerEnrollment {
                host: &host,
                config: &host.config,
                store: &store,
                account: &host.service.account,
                auth: &host.auth_service,
                worker: &host.worker_registration,
                execution: host.worker.as_ref().unwrap(),
                rpc: host.worker_rpc.as_ref().unwrap(),
                device_name: "delayed administrator",
                worker_failures: &host.worker_failures,
            }
            .enroll_worker(None, Some(&other))
            .await;
            assert!(result
                .unwrap_err()
                .is_code(talos_error::ErrorCode::PermissionDenied));
            assert!(gateway.get_node(ADMIN_TOKEN, &id).await.unwrap().available);
        }
        let environment = coordinator
            .worker_environment("embedded", "server-admin")
            .await
            .unwrap();
        let workspace = environment
            .workspace_info(agent_protocol::WorkerEnvironmentContext {
                user_id: "server-admin".into(),
                username: None,
                workspace_id: None,
            })
            .await
            .unwrap();
        assert!(!workspace.workspace_id.is_empty());
        // Simulate the previous self-outbound configuration without replacing its
        // identity. The next startup must upgrade this same node to direct RPC.
        if iteration == 0 {
            gateway
                .update_node(
                    ADMIN_TOKEN,
                    talos_types::UpdateNodeRequest {
                        node_id: id.clone(),
                        display_name: node.display_name,
                        endpoint: None,
                        status: node.status,
                    },
                )
                .await
                .unwrap();
        }
        expected_id = Some(id);
        expected_credential = Some(credential.clone());
        handle.shutdown().await.unwrap();
        task.await.unwrap().unwrap();
        if iteration == 0 {
            let mut legacy = runner;
            legacy.transport = Some(AgentNodeTransport::Outbound);
            talos_config::YamlConfigStore::new(config.source_path())
                .save_node_identity(&legacy, std::str::from_utf8(&credential).unwrap().trim())
                .unwrap();
        } else if iteration == 1 {
            // A completed credential save followed by a crash before state cleanup
            // must not require login, rotation or a new Computer on the next boot.
            std::fs::write(
                directory.path().join(".worker-state.json"),
                r#"{"enrollment_pending":true}"#,
            )
            .unwrap();
            std::fs::write(directory.path().join(".worker-state.lock"), "").unwrap();
        }
    }
    auth_server.abort();
}

#[tokio::test]
async fn server_without_local_agents_or_model_keys_manages_network_worker() {
    let directory = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let auth_server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/auth/introspect", post(introspect)),
        )
        .await
        .unwrap();
    });
    let paths = TalosPaths::from_home(directory.path().to_owned());
    paths.ensure_base_dirs().unwrap();
    let mut config = Config::default().with_runtime_paths(paths);
    config.apply_derived_paths();
    config.gateway.auth = true;
    config.gateway.host = "127.0.0.1".into();
    config.gateway.port = port().port();
    config.cloud.api_base_url = api;
    config.sync.enabled = Some(false);
    config.agent_node.embedded_worker = false;
    config.agent.agents = vec![talos_config::AgentDefinitionConfig {
        id: "default".into(),
        name: "Routing identity".into(),
        enabled: false,
        description: None,
        provider: None,
    }];
    config.agent.inner_model = None;
    config.llm.anthropic.api_key = None;
    config.llm.anthropic.models.clear();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let cert = directory.path().join("server.crt");
    let key = directory.path().join("server.key");
    std::fs::write(&cert, certificate.cert.pem()).unwrap();
    std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
    let rpc = port();
    config.agent_node.control_plane = Some(talos_config::AgentNodeControlConfig {
        bind_address: rpc.to_string(),
        tls_certificate_file: cert.clone(),
        tls_private_key_file: key,
    });
    config.cloud.agent_base_url = format!("https://{rpc}");
    config.cloud.agent_trusted_ca_file = Some(cert.clone());
    let mut document = serde_json::to_value(&config).unwrap();
    document["logging"].as_object_mut().unwrap().remove("file");
    document["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    std::fs::write(config.source_path(), serde_json::to_vec(&document).unwrap()).unwrap();
    let migration =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    let report = startup::FinishStartupReport {
        resources: talos_upgrade::RuntimeResourcesReport {
            database_migrations: talos_config::DatabaseMigrationsInstallReport {
                source_dir: migration.clone(),
                target_dir: migration.clone(),
                backend_dir: migration,
                database_type: talos_config::DatabaseType::Sqlite,
                changed: false,
            },
        },
    };
    let server = start_server_after_startup(config.reload().unwrap(), None, &report)
        .await
        .unwrap();
    assert!(server.host.worker.is_none());
    assert!(server.host.worker_rpc.is_none());
    let host = server.host.clone();
    let coordinator = server.coordinator.clone();
    let handle = server.handle();
    let task = tokio::spawn(server.run());
    wait_for("management-only Server RPC ready", async || {
        tokio::net::TcpStream::connect(rpc).await.is_ok()
    })
    .await;
    let gateway = agent_client::AgentNodeClient::connect(
        &config.cloud.agent_base_url,
        agent_client::AgentTlsTrust::Certificate(std::fs::read(cert).unwrap()),
    )
    .await
    .unwrap();
    let created = gateway
        .create_node(
            ADMIN_TOKEN,
            talos_types::CreateNodeRequest {
                display_name: "Network Worker".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .await
        .unwrap();
    let id = created.node.node_id;
    let registration = gateway
        .register(agent_protocol::NodeRequest {
            auth: agent_protocol::NodeAuth {
                node_id: id.clone(),
                token: created.credential.token,
            },
            params: agent_protocol::RegisterParams {
                product_owner: talos_types::NodeProductOwner::Computer,
                deployments: vec![agent_protocol::Deployment {
                    agent_id: "default".into(),
                }],
                system_info: None,
            },
        })
        .await
        .unwrap();
    gateway
        .heartbeat(agent_protocol::NodeRequest {
            auth: agent_protocol::NodeAuth {
                node_id: id.clone(),
                token: registration.lease_token.clone(),
            },
            params: agent_protocol::HeartbeatParams {
                health: talos_types::NodeHealth::Healthy,
            },
        })
        .await
        .unwrap();
    let auth = agent_protocol::NodeAuth {
        node_id: id.clone(),
        token: registration.lease_token.clone(),
    };
    let adapter = agent_client::AgentHarnessRpcAdapter::new(
        id.clone(),
        Arc::new(std::sync::RwLock::new(config.clone())),
        Vec::new(),
    );
    adapter
        .install_registration(
            registration,
            tokio::time::Instant::now() + std::time::Duration::from_secs(60),
        )
        .unwrap();
    let worker_stop = tokio_util::sync::CancellationToken::new();
    let transport = gateway.clone();
    let stop = worker_stop.clone();
    let worker = tokio::spawn(async move {
        transport
            .run_execution_connection(auth, adapter, stop)
            .await
    });
    wait_for("network Worker published", async || {
        gateway
            .get_node(ADMIN_TOKEN, &id)
            .await
            .is_ok_and(|node| node.available)
    })
    .await;
    assert_eq!(
        coordinator
            .resolve_agent(talos_coordinator::AgentSelectionParams::for_session_source(
                talos_types::talos::SessionSource::http(),
                "server-admin".into(),
            ),)
            .await
            .unwrap(),
        "default"
    );
    let registered = gateway.get_node(ADMIN_TOKEN, &id).await.unwrap();
    assert_eq!(registered.owner_user_id.as_deref(), Some("server-admin"));
    assert!(registered.available);
    let computers = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/v1/computers",
            config.gateway.port
        ))
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert!(computers.status().is_success());
    let body: Value = computers.json().await.unwrap();
    assert_eq!(body["data"]["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"]["data"][0]["nodeId"], id);
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}", config.gateway.port);
    let status: Value = client
        .get(format!("{url}/v1/sync/status"))
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(status["data"]["localComputerId"].is_null());
    let policy: Value = client
        .put(format!("{url}/v1/config"))
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({"type":"sessionMessageSendPolicy","value":"steer"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(policy["status"], "success");
    let policy: Value = client
        .get(format!("{url}/v1/config/sessionMessageSendPolicy"))
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(policy["data"]["policy"], "steer");
    let jobs: Value = client
        .post(format!("{url}/v1/builtin-job/list"))
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({"computerId":"embedded"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(jobs["status"], "success");
    let job = &jobs["data"]["data"][0];
    assert_eq!(job["computerId"], job["executionInstanceId"]);
    for action in ["pause", "resume"] {
        let result: Value = client
            .post(format!("{url}/v1/builtin-job/control"))
            .bearer_auth(ADMIN_TOKEN)
            .json(&json!({
                "computerId":job["computerId"], "executionInstanceId":job["executionInstanceId"],
                "jobId":job["id"], "action":action,
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["data"]["paused"], action == "pause");
    }
    gateway.list_computers(TOKEN, None).await.unwrap();
    let denied = client
        .post(format!("{url}/v1/builtin-job/list"))
        .bearer_auth(TOKEN)
        .json(&json!({"computerId":"embedded"}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
    let denied = client
        .get(format!("{url}/v1/config/sessionMessageSendPolicy"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);

    assert!(host.worker_registration.lock().await.is_none());
    assert!(config.reload().unwrap().agent_node.runner.is_none());
    worker_stop.cancel();
    worker.await.unwrap().unwrap();
    handle.shutdown().await.unwrap();
    task.await.unwrap().unwrap();
    auth_server.abort();
}
