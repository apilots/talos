//! Central administrator claim uses direct RPC and preserves personal ownership.
use super::*;
use agent_protocol::WorkerEnvironment;
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};
use std::path::Path;
use talos_config::{AgentNodeTransport, Config, TalosPaths};

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
        trusted_node_ca_file: Some(cert.clone()),
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
        let sync_host = server.sync_host.clone();
        let nodes = server.sync_host.service.nodes.clone();
        let worker_failures = server.sync_host.worker_failures.subscribe();
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
        sync_host.schedule_refresh(
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
            sync_host
                .auth_service
                .verify_access_token(token)
                .await
                .unwrap();
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
            let other = sync_host
                .auth_service
                .verify_access_token(OTHER_ADMIN_TOKEN)
                .await
                .unwrap();
            let store = talos_config::YamlConfigStore::new(config.source_path());
            let result = super::desktop_worker::WorkerEnrollment {
                sync_host: &sync_host,
                config: &sync_host.config,
                store: &store,
                account: &sync_host.service.account,
                auth: &sync_host.auth_service,
                worker: &sync_host.worker_registration,
                execution: &sync_host.worker,
                rpc: &sync_host.worker_rpc,
                device_name: "delayed administrator",
                worker_failures: &sync_host.worker_failures,
            }
            .enroll_worker(None, Some(&other))
            .await;
            assert!(result
                .unwrap_err()
                .is_code(talos_error::ErrorCode::PermissionDenied));
            assert!(gateway.get_node(ADMIN_TOKEN, &id).await.unwrap().available);
        }
        let (connection, auth) = gateway
            .connect_coordinator(ADMIN_TOKEN, &uuid::Uuid::new_v4().to_string(), &id)
            .await
            .unwrap();
        let environment =
            agent_client::AgentEnvironmentClient::from_connection(connection.clone(), auth);
        let workspace = environment
            .workspace_info(agent_protocol::WorkerEnvironmentContext {
                user_id: "server-admin".into(),
                username: None,
                workspace_id: None,
            })
            .await
            .unwrap();
        assert!(!workspace.workspace_id.is_empty());
        connection.close();
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
