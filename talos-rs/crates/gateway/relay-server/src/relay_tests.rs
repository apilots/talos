use super::*;
use agent_protocol::{CreateNodeRequest, Deployment, NodeAuth, RegisterParams};
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use talos_app_protocol::relay::{
    HostToRelay, RelayHeader, RelayResponseHead, RelayToHost, RELAY_CHUNK_BYTES, RELAY_VERSION,
    RELAY_WINDOW_FRAMES,
};
use talos_storage::SqliteStorage;
use talos_store::node::DatabaseAgentNodeStore;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

type HostSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
struct Fixture {
    relay: RelayServer,
    nodes: AgentNodeService,
    auth: NodeAuth,
    owner: Actor,
    endpoint: String,
    shutdown: CancellationToken,
    _home: tempfile::TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}
impl Fixture {
    async fn new(role: talos_types::NodeProductOwner) -> Self {
        let home = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(home.path().join("directory.db").to_str().unwrap())
            .await
            .unwrap();
        let migrations = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../conf/migrations/sqlite");
        talos_store::migration::run_migrations(storage.pool(), &migrations)
            .await
            .unwrap();
        let nodes = AgentNodeService::new(Arc::new(DatabaseAgentNodeStore::new(&storage)));
        let owner = Actor::regular("alice");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let created = nodes
            .create_node(
                &owner,
                CreateNodeRequest {
                    display_name: "Personal computer".into(),
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
        let lease = nodes
            .register(
                &principal,
                RegisterParams {
                    product_owner: role,
                    deployments: vec![Deployment {
                        agent_id: "default".into(),
                    }],
                    system_info: None,
                },
            )
            .await
            .unwrap();
        let auth = NodeAuth {
            node_id: credential.node_id,
            token: lease.lease_token,
        };
        let relay = RelayServer::new(nodes.clone());
        let shutdown = CancellationToken::new();
        let router = relay.routes(shutdown.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let ending = shutdown.clone();
        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(ending.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            relay,
            nodes,
            auth,
            owner,
            endpoint: format!("ws://{address}/talos/product/connect"),
            shutdown,
            _home: home,
        }
    }
    async fn host(&self) -> HostSocket {
        let (mut socket, _) = tokio_tungstenite::connect_async(&self.endpoint)
            .await
            .unwrap();
        send(
            &mut socket,
            HostToRelay::Hello {
                version: RELAY_VERSION,
                host_id: "host".into(),
                auth: self.auth.clone(),
            },
        )
        .await;
        assert!(matches!(
            next(&mut socket).await,
            RelayToHost::Ready {
                version: RELAY_VERSION
            }
        ));
        socket
    }
    fn request(
        &self,
        path: &str,
        body: Body,
    ) -> tokio::task::JoinHandle<TalosResult<axum::http::Response<Body>>> {
        let relay = self.relay.clone();
        let owner = self.owner.clone();
        let computer = self.auth.node_id.clone();
        let path = path.to_owned();
        tokio::spawn(async move {
            relay
                .forward(
                    &owner,
                    &computer,
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .body(body)
                        .unwrap(),
                )
                .await
        })
    }
}
async fn send(socket: &mut HostSocket, message: HostToRelay) {
    message.validate().unwrap();
    socket
        .send(Message::Text(
            serde_json::to_string(&message).unwrap().into(),
        ))
        .await
        .unwrap();
}
async fn next(socket: &mut HostSocket) -> RelayToHost {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let raw = match frame {
            Message::Text(raw) => raw.as_bytes().to_vec(),
            Message::Binary(raw) => raw.to_vec(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("unexpected socket message {other:?}"),
        };
        return serde_json::from_slice(&raw).unwrap();
    }
}
async fn next_request(socket: &mut HostSocket) -> u64 {
    loop {
        if let RelayToHost::Request { head } = next(socket).await {
            return head.stream_id;
        }
    }
}
async fn reply(socket: &mut HostSocket, id: u64, body: &[u8]) {
    send(
        socket,
        HostToRelay::ResponseHead {
            head: RelayResponseHead {
                stream_id: id,
                status: 201,
                headers: vec![RelayHeader {
                    name: "content-type".into(),
                    value: "text/plain".into(),
                }],
            },
        },
    )
    .await;
    send(
        socket,
        HostToRelay::Data {
            stream_id: id,
            sequence: 1,
            data: body.to_vec(),
        },
    )
    .await;
    send(socket, HostToRelay::End { stream_id: id }).await;
}
#[tokio::test]
async fn forwards_existing_http_headers_and_bounded_body_without_business_storage() {
    let fixture = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let mut host = fixture.host().await;
    let response = fixture.request("/v1/session", Body::from("typed-product-body"));
    let id = next_request(&mut host).await;
    let mut received = Vec::new();
    loop {
        match next(&mut host).await {
            RelayToHost::Data {
                stream_id,
                sequence,
                data,
            } => {
                assert_eq!(stream_id, id);
                received.extend(data);
                send(
                    &mut host,
                    HostToRelay::Window {
                        stream_id: id,
                        acknowledged: sequence,
                    },
                )
                .await;
            }
            RelayToHost::End { stream_id } if stream_id == id => break,
            _ => {}
        }
    }
    assert_eq!(received, b"typed-product-body");
    reply(&mut host, id, b"product-result").await;
    let response = response.await.unwrap().unwrap();
    assert_eq!(response.status(), 201);
    assert_eq!(response.headers()["content-type"], "text/plain");
    assert_eq!(
        to_bytes(response.into_body(), RELAY_CHUNK_BYTES)
            .await
            .unwrap()
            .as_ref(),
        b"product-result"
    );
}
#[tokio::test]
async fn slow_file_response_does_not_block_an_independent_interactive_request() {
    let fixture = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let mut host = fixture.host().await;
    let first = fixture.request("/v1/files/download", Body::empty());
    let first_id = next_request(&mut host).await;
    send(
        &mut host,
        HostToRelay::ResponseHead {
            head: RelayResponseHead {
                stream_id: first_id,
                status: 200,
                headers: vec![],
            },
        },
    )
    .await;
    let held_response = first.await.unwrap().unwrap();
    for sequence in 1..=RELAY_WINDOW_FRAMES {
        send(
            &mut host,
            HostToRelay::Data {
                stream_id: first_id,
                sequence,
                data: vec![7; RELAY_CHUNK_BYTES],
            },
        )
        .await;
    }
    let second = fixture.request("/v1/session/status", Body::empty());
    let second_id = next_request(&mut host).await;
    assert_ne!(first_id, second_id);
    reply(&mut host, second_id, b"interactive-ready").await;
    let response = tokio::time::timeout(Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        to_bytes(response.into_body(), RELAY_CHUNK_BYTES)
            .await
            .unwrap()
            .as_ref(),
        b"interactive-ready"
    );
    drop(held_response);
}
#[tokio::test]
async fn ownership_and_cloud_role_are_checked_before_opening_a_relay_stream() {
    let personal = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let denied = personal
        .relay
        .forward(
            &Actor::regular("bob"),
            &personal.auth.node_id,
            Request::new(Body::empty()),
        )
        .await;
    assert_eq!(
        denied.unwrap_err().error_code(),
        ErrorCode::ResourceNotFound
    );
    let _attached = personal.host().await;
    assert!(personal
        .relay
        .forward(
            &Actor::regular("bob").with_role(talos_auth::UserRole::Admin),
            &personal.auth.node_id,
            Request::new(Body::empty())
        )
        .await
        .is_err());
    let cloud = Fixture::new(talos_types::NodeProductOwner::Server).await;
    let denied = cloud
        .relay
        .forward(
            &cloud.owner,
            &cloud.auth.node_id,
            Request::new(Body::empty()),
        )
        .await;
    assert_eq!(denied.unwrap_err().error_code(), ErrorCode::InvalidRequest);
}
#[tokio::test]
async fn lease_revocation_fails_pending_delivery_without_replacing_worker_publication() {
    let fixture = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let mut host = fixture.host().await;
    let pending = fixture.request("/v1/session", Body::empty());
    next_request(&mut host).await;
    let principal = fixture
        .nodes
        .authenticate_lease(&fixture.auth)
        .await
        .unwrap();
    fixture.nodes.unregister(&principal).await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::ServiceUnavailable);
}

#[tokio::test]
async fn source_peer_alias_is_account_scoped_and_duplicate_live_hosts_are_rejected() {
    let fixture = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let _first = fixture.host().await;
    assert_eq!(
        fixture
            .relay
            .resolve_source_peer(&fixture.owner, "host")
            .await
            .unwrap(),
        fixture.auth.node_id
    );
    assert!(fixture
        .relay
        .resolve_source_peer(&Actor::regular("bob"), "host")
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    let created = fixture
        .nodes
        .create_node(
            &fixture.owner,
            CreateNodeRequest {
                display_name: "Second computer".into(),
                endpoint: None,
                owner_user_id: None,
                credential_expires_at: chrono_now() + 3600,
            },
        )
        .await
        .unwrap();
    let auth = NodeAuth {
        node_id: created.credential.node_id,
        token: created.credential.token,
    };
    let principal = fixture
        .nodes
        .authenticate_registration(&auth)
        .await
        .unwrap();
    let registration = fixture
        .nodes
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
    let (mut second, _) = tokio_tungstenite::connect_async(&fixture.endpoint)
        .await
        .unwrap();
    send(
        &mut second,
        HostToRelay::Hello {
            version: RELAY_VERSION,
            host_id: "host".into(),
            auth: NodeAuth {
                node_id: auth.node_id,
                token: registration.lease_token,
            },
        },
    )
    .await;
    assert!(matches!(next(&mut second).await, RelayToHost::Ready { .. }));
    assert!(fixture
        .relay
        .resolve_source_peer(&fixture.owner, "host")
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    let principal = fixture
        .nodes
        .authenticate_lease(&fixture.auth)
        .await
        .unwrap();
    fixture.nodes.unregister(&principal).await.unwrap();
    assert_ne!(
        fixture
            .relay
            .resolve_source_peer(&fixture.owner, "host")
            .await
            .unwrap(),
        fixture.auth.node_id
    );
}
fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test]
async fn silent_product_socket_expires_while_node_lease_heartbeats_remain_valid() {
    let fixture = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let mut host = fixture.host().await;
    let pending = fixture.request("/v1/waiting", Body::empty());
    next_request(&mut host).await;
    loop {
        let frame = host.next().await.unwrap().unwrap();
        if matches!(frame, Message::Ping(_)) {
            break;
        }
        assert!(matches!(frame, Message::Text(_)));
    }
    // Do not poll or flush this socket again: its Pong stays unsent.
    let principal = fixture
        .nodes
        .authenticate_lease(&fixture.auth)
        .await
        .unwrap();
    fixture
        .nodes
        .heartbeat(
            &principal,
            agent_protocol::HeartbeatParams {
                health: talos_types::NodeHealth::Healthy,
            },
        )
        .await
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(40), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.is_code(ErrorCode::ServiceUnavailable));
    assert!(fixture
        .nodes
        .authenticate_lease(&fixture.auth)
        .await
        .is_ok());
    assert!(fixture
        .relay
        .resolve_source_peer(&fixture.owner, "host")
        .await
        .is_err());
}

#[tokio::test]
async fn generic_file_hint_uses_file_lane_without_forwarding_account_credentials() {
    let fixture = Fixture::new(talos_types::NodeProductOwner::Computer).await;
    let mut host = fixture.host().await;
    for (hint, expected) in [
        ("true", talos_app_protocol::relay::RelayLane::File),
        ("control", talos_app_protocol::relay::RelayLane::Interactive),
    ] {
        let relay = fixture.relay.clone();
        let owner = fixture.owner.clone();
        let computer = fixture.auth.node_id.clone();
        let pending = tokio::spawn(async move {
            relay
                .forward(
                    &owner,
                    &computer,
                    Request::builder()
                        .method("POST")
                        .uri("/v1/app/request")
                        .header("authorization", "Bearer test-private-token")
                        .header("cookie", "test-private-cookie")
                        .header(talos_app_protocol::relay::RELAY_FILE_TRANSFER_HEADER, hint)
                        .body(Body::from("typed body"))
                        .unwrap(),
                )
                .await
        });
        let head = loop {
            if let RelayToHost::Request { head } = next(&mut host).await {
                break head;
            }
        };
        assert_eq!(head.lane, expected);
        assert!(head.headers.iter().all(|header| !matches!(
            header.name.as_str(),
            "authorization" | "cookie" | "x-talos-file-transfer"
        )));
        assert_eq!(head.identity.0.user_id, fixture.owner.user_id);
        assert!(head.identity.0.access_token.is_none());
        assert!(!head.identity.0.auth_disabled);
        assert_eq!(
            head.identity.0.request_origin,
            talos_auth::ProductRequestOrigin::RemoteProduct
        );
        reply(&mut host, head.stream_id, b"done").await;
        let response = pending.await.unwrap().unwrap();
        assert_eq!(to_bytes(response.into_body(), 1024).await.unwrap(), "done");
    }
}
