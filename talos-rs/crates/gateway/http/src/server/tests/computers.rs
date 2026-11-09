use super::*;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn computer_http_management_preserves_identity_and_redacts_directory_secrets() {
    let state = test_state(false).await;
    // Exercise the actual Computer routes and authentication middleware with real node storage.
    let app = super::super::computers::routes()
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let (status, _, _) = call(
        &app,
        "GET",
        "/v1/computers",
        /* token */ None,
        serde_json::json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, headers, created) = call(
        &app,
        "POST",
        "/v1/computers",
        Some("owner"),
        serde_json::json!({
            "displayName": "Laptop", "credentialExpiresAt": chrono::Utc::now().timestamp() + 3600
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    let node_id = created["data"]["node"]["nodeId"].as_str().unwrap();
    let credential = created["data"]["credential"]["token"].as_str().unwrap();
    assert!(!node_id.is_empty());
    assert!(!credential.is_empty());
    let path = format!("/v1/computers/{node_id}");
    let (status, _, _) = call(&app, "GET", &path, Some("other"), serde_json::json!(null)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, page) = call(
        &app,
        "GET",
        "/v1/computers?query=laptop&limit=1",
        Some("owner"),
        serde_json::json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["data"]["total"], 1);
    assert_eq!(page["data"]["data"][0]["nodeId"], node_id);
    assert!(!page.to_string().contains(credential));
    assert_eq!(page["data"]["data"][0]["available"], false);
    let (status, _, renamed) = call(
        &app,
        "PATCH",
        &format!("{path}/name"),
        Some("owner"),
        serde_json::json!({
            "nodeId": node_id, "displayName": "Renamed laptop"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["data"]["nodeId"], node_id);
    let (status, _, disabled) = call(
        &app,
        "PUT",
        &format!("{path}/status"),
        Some("owner"),
        serde_json::json!({
            "nodeId": node_id, "status": "disabled"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled["data"]["displayName"], "Renamed laptop");
    assert_eq!(disabled["data"]["status"], "disabled");
    let (status, headers, rotated) = call(
        &app,
        "POST",
        &format!("{path}/credential"),
        Some("owner"),
        serde_json::json!({
            "nodeId": node_id, "expiresAt": chrono::Utc::now().timestamp() + 7200
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(rotated["data"]["nodeId"], node_id);
    assert_ne!(rotated["data"]["token"], credential);
    let (status, _, read) = call(&app, "GET", &path, Some("owner"), serde_json::json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["data"]["nodeId"], node_id);
    assert!(!read.to_string().contains(credential));
    assert!(!read
        .to_string()
        .contains(rotated["data"]["token"].as_str().unwrap()));
}

#[tokio::test]
async fn managed_worker_read_uses_its_static_route_on_an_unconfigured_server() {
    let state = test_state(false).await;
    let app = super::super::computers::routes()
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let (status, _, body) = call(
        &app,
        "GET",
        "/v1/computers/managed-worker",
        Some("owner"),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["data"],
        serde_json::json!({ "enabled": false, "instance": null })
    );
}

#[tokio::test]
async fn worker_runtime_settings_require_the_authenticated_server_admin() {
    let state = test_state(false).await;
    let routes = super::super::worker_runtime::routes().with_state(state.clone());
    let app = routes
        .clone()
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for (method, path) in [
        ("GET", "/v1/worker-runtime"),
        ("POST", "/v1/worker-runtime/validate"),
        ("PUT", "/v1/worker-runtime"),
    ] {
        let (status, _, body) = call(
            &app,
            method,
            path,
            Some("owner"),
            serde_json::json!({
                "kind": "kubernetes",
                "namespace": "talos",
                "storageClass": "local-path",
                "authentication": { "kind": "pasted", "kubeconfig": "private-fixture-token" }
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!body.to_string().contains("private-fixture-token"));
    }

    let admin = RequestIdentity {
        role: talos_auth::UserRole::Admin,
        access_token: Some("admin-token".into()),
        ..test_auth()
    };
    let (status, headers, body) = call(
        &routes.layer(axum::Extension(admin)),
        "GET",
        "/v1/worker-runtime",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(
        body["data"],
        serde_json::json!({
            "activeBackend": null,
            "configuredBackend": null,
            "dockerSocketPath": null,
            "profile": null,
            "timing": null,
            "kubernetes": null,
            "restartRequired": false
        })
    );
}

#[tokio::test]
async fn socket_enabled_server_guards_default_and_explicit_embedded_workspace_access() {
    let mut config = test_config();
    config.talos.worker.runtime = Some(talos_config::WorkerRuntimeConfig {
        kind: talos_config::domains::managed_worker::WorkerRuntimeKind::DockerEngine,
        image: "example.test/worker@sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
        control_plane_endpoint: "https://agent.example.test".into(),
        trusted_ca_file: None,
        docker_engine: Default::default(),
        kubernetes: Default::default(),
    });
    let state = test_state_with_config(config, false).await;
    let app = Router::new()
        .route(
            "/v1/workspace/info",
            axum::routing::get(workspace_info_handler),
        )
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    for path in ["/v1/workspace/info", "/v1/workspace/info?nodeId=embedded"] {
        let (status, _, _) = call(&app, "GET", path, Some("owner"), Value::Null).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn held_http_deletion_isolates_other_computer_reads_and_lifecycle() {
    use agent_protocol::{
        HeartbeatParams, NodeAuth, NodeHealth, RegisterParams, RegistrationAdmission,
    };
    use std::time::Duration;
    use tokio::sync::Notify;

    let state = test_state(false).await;
    let nodes = state.app.nodes.clone();
    let app = super::super::computers::routes()
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let owner = talos_service::Actor::regular("owner");
    let create = |name: &'static str| {
        nodes.create_node(
            &owner,
            talos_service::CreateNodeRequest {
                display_name: name.into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
    };
    let first = create("First Computer").await.unwrap();
    let second = create("Other Computer").await.unwrap();
    let first_id = first.node.node_id;
    let second_id = second.node.node_id;
    let register = |credential: NodeAuth| {
        let nodes = nodes.clone();
        async move {
            let _lifecycle = nodes.lock_lifecycle(&credential.node_id).await;
            let principal = nodes.authenticate_registration(&credential).await.unwrap();
            let registered = nodes
                .register(
                    &principal,
                    RegisterParams {
                        system_info: None,
                        deployments: vec![agent_protocol::Deployment {
                            agent_id: "default".into(),
                        }],
                    },
                )
                .await
                .unwrap();
            let lease = NodeAuth {
                node_id: credential.node_id,
                token: registered.lease_token,
            };
            nodes
                .mark_registration_published(&lease, Arc::new(|| Box::pin(async {})))
                .await
                .unwrap();
            lease
        }
    };
    let first_auth = NodeAuth {
        node_id: first_id.clone(),
        token: first.credential.token,
    };
    let first_lease = register(first_auth).await;
    let first_principal = nodes.authenticate_lease(&first_lease).await.unwrap();
    let second_lease = register(NodeAuth {
        node_id: second_id.clone(),
        token: second.credential.token,
    })
    .await;
    let second_principal = nodes.authenticate_lease(&second_lease).await.unwrap();
    let healthy = || HeartbeatParams {
        health: NodeHealth::Healthy,
    };
    nodes.heartbeat(&second_principal, healthy()).await.unwrap();

    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let hook_entered = entered.clone();
    let hook_release = release.clone();
    nodes
        .set_registration_deletion_hook(
            &first_lease,
            Arc::new(move |admission| {
                let entered = hook_entered.clone();
                let release = hook_release.clone();
                Box::pin(async move {
                    if admission == RegistrationAdmission::Block {
                        entered.notify_one();
                        release.notified().await;
                    }
                    Ok(())
                })
            }),
        )
        .await
        .unwrap();

    let delete_app = app.clone();
    let first_path = format!("/v1/computers/{first_id}");
    let deletion = tokio::spawn(async move {
        call(
            &delete_app,
            "DELETE",
            &first_path,
            Some("owner"),
            Value::Null,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .expect("HTTP deletion entered remote preparation");
    let mut renewal = Box::pin(nodes.renew_lease(&first_principal));
    assert!(futures_util::poll!(&mut renewal).is_pending());

    tokio::time::timeout(Duration::from_secs(3), async {
        let path = format!("/v1/computers/{second_id}");
        let (status, _, read) = call(&app, "GET", &path, Some("owner"), Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read["data"]["available"], true);
        let heartbeat = nodes.heartbeat(&second_principal, healthy()).await.unwrap();
        assert!(!heartbeat.registration_required);
        let (status, _, rotated) = call(
            &app,
            "POST",
            &format!("{path}/credential"),
            Some("owner"),
            serde_json::json!({ "nodeId": second_id, "expiresAt": chrono::Utc::now().timestamp() + 7200 }),
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(rotated["data"]["nodeId"], second_id);
    }).await.expect("the other Computer can be read, send a heartbeat and rotate its credential");
    assert!(!deletion.is_finished());

    release.notify_one();
    let (status, _, deleted) = deletion.await.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(deleted["data"]["nodeId"], first_id);
    assert!(renewal
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Unauthenticated));
}
