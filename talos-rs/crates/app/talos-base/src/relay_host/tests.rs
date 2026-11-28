use super::*;
use agent_protocol::{CreateNodeRequest, Deployment, NodeAuth, RegisterParams};
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::post,
};
use talos_auth::Actor;
use talos_control_plane::AgentNodeService;
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn outbound_host_dispatches_the_existing_router_and_streams_both_windows() {
    let home = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(home.path().join("directory.db").to_str().unwrap())
        .await
        .unwrap();
    let migrations =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    talos_store::migration::run_migrations(storage.pool(), &migrations)
        .await
        .unwrap();
    let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
    let owner = Actor::regular("relay-host-owner");
    let now = chrono::Utc::now().timestamp();
    let created = nodes
        .create_node(
            &owner,
            CreateNodeRequest {
                display_name: "Personal".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: now + 3600,
            },
        )
        .await
        .unwrap();
    let credential = NodeAuth {
        node_id: created.credential.node_id,
        token: created.credential.token,
    };
    let principal = nodes.authenticate_registration(&credential).await.unwrap();
    let registration = nodes
        .register(
            &principal,
            RegisterParams {
                product_owner: talos_types::NodeProductOwner::Computer,
                deployments: vec![Deployment {
                    agent_id: "default".into(),
                }],
                system_info: None,
            },
        )
        .await
        .unwrap();
    let relay = talos_relay_server::RelayServer::new(nodes.clone());
    let shutdown = CancellationToken::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server_router = relay.routes(shutdown.clone());
    let ending = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, server_router)
            .with_graceful_shutdown(ending.cancelled_owned())
            .await
            .unwrap()
    });
    let host_router = Router::new().route(
        "/same-api",
        post(|request: Request<Body>| async {
            let bytes = to_bytes(request.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap();
            ([("x-existing-api", "local")], bytes)
        }),
    );
    let (lease_tx, leases) = watch::channel(Some(AgentNodeLeaseUpdate {
        auth: NodeAuth {
            node_id: credential.node_id.clone(),
            token: registration.lease_token,
        },
        owner_user_id: owner.user_id.clone(),
        validity: registration.validity,
        deadline: tokio::time::Instant::now() + Duration::from_secs(300),
    }));
    let host = spawn(
        endpoint,
        AgentTlsTrust::SystemRoots,
        "host".into(),
        leases,
        host_router,
    );
    let payload = vec![b'x'; talos_app_protocol::relay::RELAY_CHUNK_BYTES * 12 + 19];
    let response = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = relay
                .forward(
                    &owner,
                    &credential.node_id,
                    Request::builder()
                        .method("POST")
                        .uri("/same-api")
                        .body(Body::from(payload.clone()))
                        .unwrap(),
                )
                .await;
            match result {
                Ok(response) => break response,
                Err(error) if error.is_code(ErrorCode::ServiceUnavailable) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(error) => panic!("Unexpected relay error: {:?}", error.error_code()),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(response.headers()["x-existing-api"], "local");
    assert_eq!(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
        payload
    );
    lease_tx.send(None).unwrap();
    host.abort();
    shutdown.cancel();
    server.await.unwrap();
}

#[tokio::test]
async fn aborting_host_delivery_cancels_all_child_stream_scopes() {
    let token = CancellationToken::new();
    let seen = token.clone();
    let task = tokio::spawn(async move {
        let _guard = ConnectionLifetime(token);
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    task.abort();
    let _ = task.await;
    assert!(seen.is_cancelled());
}

#[test]
fn product_endpoint_requires_verified_tls_outside_loopback() {
    assert_eq!(
        product_endpoint("https://talos.example").unwrap(),
        "https://talos.example/talos/product/connect"
    );
    assert!(product_endpoint("http://127.0.0.1:8080").is_ok());
    assert!(product_endpoint("http://[::1]:8080").is_ok());
    for endpoint in [
        "http://talos.example",
        "https://user:password@talos.example",
        "https://talos.example?token=secret",
        "https://talos.example#fragment",
    ] {
        assert!(product_endpoint(endpoint)
            .unwrap_err()
            .is_code(ErrorCode::InvalidRequest));
    }
}

#[tokio::test]
async fn host_rejects_delegated_identity_outside_its_current_lease_owner() {
    use talos_app_protocol::relay::{RelayLane, RelayRequestHead, RelayRequestIdentity};
    let (connection, _outgoing) = Connection::new("lease-owner".into(), "personal-host".into());
    let result = request::dispatch(
        &connection,
        &Router::new(),
        RelayToHost::Request {
            head: RelayRequestHead {
                stream_id: 1,
                identity: RelayRequestIdentity(talos_auth::Actor::regular("another-account")),
                method: "GET".into(),
                path_and_query: "/v1/files".into(),
                headers: vec![],
                lane: RelayLane::Interactive,
                callback_origin: None,
            },
        },
    )
    .await;
    assert!(result.unwrap_err().is_code(ErrorCode::PermissionDenied));
    assert!(connection.streams.lock().await.is_empty());
}
