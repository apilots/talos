//! Controlled TLS ingress regression, using the real directory and publication owner.
use super::*;
use agent_client::AgentTlsTrust;
use agent_protocol::{Deployment, NodeAuth};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, ClientTlsConfig};

struct Ingress {
    client: pb::agent_node_control_client::AgentNodeControlClient<Channel>,
    endpoint: String,
    trust: agent_client::AgentTlsTrust,
    identity: axum_server::tls_rustls::RustlsConfig,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Ingress {
    async fn new(control: NodeControlRpc) -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let pem = certificate.cert.pem();
        let identity = axum_server::tls_rustls::RustlsConfig::from_pem(
            pem.as_bytes().to_vec(),
            certificate.signing_key.serialize_pem().into_bytes(),
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, task) = Self::serve(control, listener, identity.clone());
        let channel = tonic::transport::Endpoint::from_shared(format!("https://{address}"))
            .unwrap()
            .tls_config(
                ClientTlsConfig::new()
                    .ca_certificate(tonic::transport::Certificate::from_pem(pem.clone())),
            )
            .unwrap()
            .connect()
            .await
            .unwrap();
        Self {
            endpoint: format!("https://{address}"),
            trust: agent_client::AgentTlsTrust::Certificate(pem.into_bytes()),
            client: pb::agent_node_control_client::AgentNodeControlClient::new(channel),
            identity,
            stop,
            task,
        }
    }

    fn serve(
        control: NodeControlRpc,
        listener: tokio::net::TcpListener,
        identity: axum_server::tls_rustls::RustlsConfig,
    ) -> (CancellationToken, tokio::task::JoinHandle<()>) {
        let stop = control.shutdown.clone();
        let server_stop = stop.clone();
        let task = tokio::spawn(async move {
            let websocket = control.clone().websocket_routes();
            let routes = super::super::super::node_control_router(
                tonic::service::Routes::new(
                    pb::agent_node_control_server::AgentNodeControlServer::new(control),
                ),
                websocket,
            );
            let handle = axum_server::Handle::new();
            let signal = handle.clone();
            let server = axum_server::from_tcp_rustls(listener.into_std().unwrap(), identity)
                .unwrap()
                .handle(handle)
                .serve(routes.into_make_service());
            tokio::pin!(server);
            tokio::select! {
                result = &mut server => result.unwrap(),
                _ = server_stop.cancelled() => { signal.graceful_shutdown(Some(std::time::Duration::from_secs(1))); server.await.unwrap(); }
            }
        });
        (stop, task)
    }

    async fn restart(&mut self, control: NodeControlRpc) {
        self.stop.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(3), &mut self.task)
            .await
            .unwrap()
            .unwrap();
        let listener =
            tokio::net::TcpListener::bind(self.endpoint.strip_prefix("https://").unwrap())
                .await
                .unwrap();
        (self.stop, self.task) = Self::serve(control, listener, self.identity.clone());
    }

    async fn connect(
        &self,
        auth: pb::NodeAuth,
    ) -> Result<
        (
            mpsc::Sender<pb::NodeToControlFrame>,
            tonic::Streaming<pb::ControlToNodeFrame>,
        ),
        Status,
    > {
        let (sender, receiver) = mpsc::channel(8);
        sender
            .send(pb::NodeToControlFrame {
                body: Some(pb::node_to_control_frame::Body::Connect(
                    pb::ConnectNodeRequest {
                        auth: Some(auth),
                        params: Some(pb::Empty {}),
                        stream_heartbeat: false,
                    },
                )),
            })
            .await
            .unwrap();
        let input = self
            .client
            .clone()
            .connect(ReceiverStream::new(receiver))
            .await?
            .into_inner();
        Ok((sender, input))
    }

    async fn close(self) {
        self.stop.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(3), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

async fn ready(stream: &mut tonic::Streaming<pb::ControlToNodeFrame>) {
    let frame = stream.message().await.unwrap().unwrap();
    assert!(matches!(
        frame.body,
        Some(pb::control_to_node_frame::Body::Connected(
            pb::ConnectNodeResponse {
                result: Some(pb::connect_node_response::Result::Ready(_)),
                ..
            }
        ))
    ));
}

#[tokio::test]
async fn worker_reconnect_retains_lease_and_replacement_stops_old_runner() {
    worker_reconnect_scenario(talos_config::AgentNodeTransport::Direct).await;
}

#[tokio::test]
async fn websocket_worker_reconnect_retains_lease_and_replacement_stops_old_runner() {
    worker_reconnect_scenario(talos_config::AgentNodeTransport::Outbound).await;
}

async fn worker_reconnect_scenario(transport: talos_config::AgentNodeTransport) {
    let mut fixture = Fixture::new(/* endpoint */ None).await;
    let mut ingress = Ingress::new(fixture.control.clone()).await;
    let node_id = fixture.credential.node_id.clone();
    let home = tempfile::tempdir().unwrap();
    let adapter = talos_agent::AgentHarnessRpcAdapter::new(
        node_id.clone(),
        Arc::new(std::sync::RwLock::new(
            talos_config::Config::default()
                .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned())),
        )),
        Vec::new(),
    );
    let observed = Arc::new(std::sync::Mutex::new(
        None::<agent_client::AgentNodeLeaseUpdate>,
    ));
    let leases = observed.clone();
    let registered_adapter = adapter.clone();
    let stop = CancellationToken::new();
    let runner = AgentNodeRunner::new(
        match transport {
            talos_config::AgentNodeTransport::Outbound => {
                AgentNodeClient::connect_websocket(&ingress.endpoint, ingress.trust.clone()).await
            }
            _ => AgentNodeClient::connect(&ingress.endpoint, ingress.trust.clone()).await,
        }
        .unwrap(),
        node_id.clone(),
        fixture.credential.token.clone(),
        vec![agent_protocol::Deployment {
            agent_id: "talos".into(),
        }],
    )
    .with_lease_observer(Arc::new(move |update| {
        if let Some(lease) = &update {
            registered_adapter.install_registration(
                agent_protocol::Registration {
                    lease_token: lease.auth.token.clone(),
                    validity: lease.validity,
                    owner_user_id: lease.owner_user_id.clone(),
                },
                lease.deadline,
            )?;
        } else {
            registered_adapter.clear_registration();
        }
        *leases.lock().unwrap() = update;
        Ok(())
    }))
    .with_execution_adapter(adapter);
    let mut task = tokio::spawn(runner.run(stop.clone()));
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(connection) = fixture
                .publisher
                .connections
                .lock()
                .await
                .get(&node_id)
                .cloned()
            {
                break connection;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let original = observed.lock().unwrap().clone().unwrap();
    fixture
        .control
        .nodes
        .revoke_publication(&original.auth)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if fixture
                .publisher
                .connections
                .lock()
                .await
                .get(&node_id)
                .is_some_and(|connection| {
                    !connection.is_closed() && !Arc::ptr_eq(connection, &first)
                })
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let recovered = observed.lock().unwrap().clone().unwrap();
    assert_eq!(recovered.auth.token, original.auth.token);
    // A restarted control plane retains the DB lease but loses its in-memory
    // publication. The existing heartbeat explicitly requests registration.
    let nodes = AgentNodeService::new(fixture.store.clone());
    let publisher = RemoteNodePublisher {
        connections: Arc::default(),
        ..fixture.publisher.clone()
    };
    let restarted = NodeControlRpc::new(nodes, publisher.clone(), CancellationToken::new());
    fixture.control = restarted.clone();
    fixture.publisher = publisher.clone();
    ingress.restart(restarted).await;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if publisher
                .connections
                .lock()
                .await
                .get(&fixture.credential.node_id)
                .is_some_and(|connection| !connection.is_closed())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("control-plane restart must restore registration");
    assert_ne!(
        observed.lock().unwrap().as_ref().unwrap().auth.token,
        original.auth.token
    );
    let replacement = registered(
        ingress
            .client
            .clone()
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    let (_sender, mut worker) = ingress
        .connect(pb::NodeAuth {
            node_id,
            token: replacement.clone(),
        })
        .await
        .unwrap();
    ready(&mut worker).await;
    let error = tokio::time::timeout(std::time::Duration::from_secs(10), &mut task)
        .await
        .expect("replaced Worker must stop rather than register again")
        .unwrap()
        .unwrap_err();
    assert!(error.is_code(ErrorCode::Unauthenticated));
    fixture.heartbeat(replacement).await;
    assert_eq!(recovered.auth.token, original.auth.token);
    assert!(observed.lock().unwrap().is_none());
    ingress.close().await;
}

#[tokio::test]
async fn connect_replaces_only_the_live_stream_and_disable_revokes_it() {
    let fixture = Fixture::new(/* endpoint */ None).await;
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    fixture.heartbeat(lease.clone()).await;
    let ingress = Ingress::new(fixture.control.clone()).await;
    let denied = ingress
        .connect(fixture.credential_auth())
        .await
        .err()
        .unwrap();
    assert!(error_pb::decode_status(denied).is_code(ErrorCode::Unauthenticated));
    let auth = pb::NodeAuth {
        node_id: fixture.credential.node_id.clone(),
        token: lease,
    };
    let (first_sender, mut first) = ingress.connect(auth.clone()).await.unwrap();
    ready(&mut first).await;
    let (_second_sender, mut second) = ingress.connect(auth.clone()).await.unwrap();
    ready(&mut second).await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), first.message())
            .await
            .unwrap()
            .unwrap()
            .is_none()
    );
    drop(first_sender);
    // Give the superseded stream's cleanup task the lifecycle lock before checking publication.
    tokio::task::yield_now().await;
    {
        let _lifecycle = fixture.control.nodes.lock_lifecycle(&auth.node_id).await;
        fixture
            .publisher
            .coordinator
            .ensure_agent("talos")
            .await
            .unwrap();
        assert!(
            fixture
                .control
                .nodes
                .get_node(
                    &Actor::regular("user-1"),
                    agent_protocol::GetNodeRequest {
                        node_id: auth.node_id.clone(),
                    }
                )
                .await
                .unwrap()
                .available
        );
    }
    fixture
        .control
        .nodes
        .set_computer_status(
            &Actor::regular("user-1"),
            talos_types::SetComputerStatusRequest {
                node_id: auth.node_id.clone(),
                status: talos_types::NodeStatus::Disabled,
            },
        )
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), second.message())
            .await
            .unwrap()
            .unwrap()
            .is_none()
    );
    assert!(fixture
        .publisher
        .coordinator
        .ensure_agent("talos")
        .await
        .is_err());
    let denied = ingress.connect(auth).await.err().unwrap();
    assert!(error_pb::decode_status(denied).is_code(ErrorCode::Unauthenticated));
    ingress.close().await;
}

#[tokio::test]
async fn server_shutdown_closes_live_connect_without_waiting_for_worker_eof() {
    let fixture = Fixture::new(/* endpoint */ None).await;
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    let ingress = Ingress::new(fixture.control.clone()).await;
    let (_sender, mut input) = ingress
        .connect(pb::NodeAuth {
            node_id: fixture.credential.node_id,
            token: lease,
        })
        .await
        .unwrap();
    ready(&mut input).await;
    ingress.close().await;
    assert!(input.message().await.unwrap().is_none());
    assert!(fixture
        .publisher
        .coordinator
        .ensure_agent("talos")
        .await
        .is_err());
}

#[tokio::test]
async fn websocket_lifecycle_preserves_auth_revocation_and_tls_validation() {
    use agent_protocol::{HeartbeatParams, NodeHealth, NodeRequest, RegisterParams};
    let fixture = Fixture::new(None).await;
    let ingress = Ingress::new(fixture.control.clone()).await;
    let client = AgentNodeClient::connect_websocket(&ingress.endpoint, ingress.trust.clone())
        .await
        .unwrap();
    let credential = NodeAuth {
        node_id: fixture.credential.node_id.clone(),
        token: fixture.credential.token.clone(),
    };
    let params = RegisterParams {
        product_owner: talos_types::NodeProductOwner::Server,
        deployments: vec![Deployment {
            agent_id: "talos".into(),
        }],
        system_info: None,
    };
    let error = client
        .register(NodeRequest {
            auth: NodeAuth {
                token: "invalid".into(),
                ..credential.clone()
            },
            params: params.clone(),
        })
        .await
        .err()
        .unwrap();
    assert!(error.is_code(ErrorCode::Unauthenticated));
    let untrusted =
        AgentNodeClient::connect_websocket(&ingress.endpoint, AgentTlsTrust::SystemRoots)
            .await
            .unwrap();
    assert!(untrusted
        .register(NodeRequest {
            auth: credential.clone(),
            params: params.clone()
        })
        .await
        .is_err());
    assert!(
        AgentNodeClient::connect_websocket("http://127.0.0.1:1", AgentTlsTrust::SystemRoots)
            .await
            .is_err()
    );
    let registration = client
        .register(NodeRequest {
            auth: credential.clone(),
            params: params.clone(),
        })
        .await
        .unwrap();
    let auth = NodeAuth {
        token: registration.lease_token,
        ..credential.clone()
    };
    client
        .renew_lease(NodeRequest {
            auth: auth.clone(),
            params: (),
        })
        .await
        .unwrap();
    client
        .heartbeat(NodeRequest {
            auth: auth.clone(),
            params: HeartbeatParams {
                health: NodeHealth::Healthy,
            },
        })
        .await
        .unwrap();
    client
        .unregister(NodeRequest {
            auth: auth.clone(),
            params: (),
        })
        .await
        .unwrap();
    assert!(client
        .renew_lease(NodeRequest { auth, params: () })
        .await
        .is_err());
    fixture
        .control
        .nodes
        .set_computer_status(
            &Actor::regular("user-1"),
            talos_types::SetComputerStatusRequest {
                node_id: credential.node_id.clone(),
                status: talos_types::NodeStatus::Disabled,
            },
        )
        .await
        .unwrap();
    assert!(client
        .register(NodeRequest {
            auth: credential,
            params
        })
        .await
        .is_err());
    ingress.close().await;
}

#[tokio::test]
async fn websocket_oversized_frame_closes_publication() {
    use futures_util::{SinkExt, StreamExt};
    use prost::Message;
    use reqwest_websocket::{Message as WsMessage, RequestBuilderExt};
    let fixture = Fixture::new(None).await;
    let ingress = Ingress::new(fixture.control.clone()).await;
    let client = AgentNodeClient::connect_websocket(&ingress.endpoint, ingress.trust.clone())
        .await
        .unwrap();
    let request = fixture.registration(&["talos"]).into_inner();
    let registration = client
        .register(agent_protocol::NodeRequest {
            auth: node_pb::decode_auth(request.auth).unwrap(),
            params: node_pb::decode_register(request.params).unwrap(),
        })
        .await
        .unwrap();
    let AgentTlsTrust::Certificate(pem) = &ingress.trust else {
        panic!("private fixture CA required")
    };
    let http = reqwest::Client::builder()
        .http1_only()
        .add_root_certificate(reqwest::Certificate::from_pem(pem).unwrap())
        .build()
        .unwrap();
    let mut socket = http
        .get(format!(
            "{}/talos/worker/connect",
            ingress.endpoint.replacen("https://", "wss://", 1)
        ))
        .upgrade()
        .send()
        .await
        .unwrap()
        .into_websocket()
        .await
        .unwrap();
    let handshake = pb::NodeToControlFrame {
        body: Some(pb::node_to_control_frame::Body::Connect(
            pb::ConnectNodeRequest {
                auth: Some(pb::NodeAuth {
                    node_id: fixture.credential.node_id.clone(),
                    token: registration.lease_token,
                }),
                params: Some(pb::Empty {}),
                stream_heartbeat: true,
            },
        )),
    };
    socket
        .send(WsMessage::Binary(handshake.encode_to_vec().into()))
        .await
        .unwrap();
    let WsMessage::Binary(bytes) = socket.next().await.unwrap().unwrap() else {
        panic!("ready frame required")
    };
    assert!(matches!(
        pb::ControlToNodeFrame::decode(bytes).unwrap().body,
        Some(pb::control_to_node_frame::Body::Connected(
            pb::ConnectNodeResponse {
                result: Some(pb::connect_node_response::Result::Ready(_)),
                ..
            }
        ))
    ));
    // Exceeds the same 4 MiB envelope used by gRPC; WSS is not an unlimited Sync upload channel.
    let _ = socket
        .send(WsMessage::Binary(vec![0; 4 * 1024 * 1024 + 1].into()))
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if fixture
                .publisher
                .coordinator
                .worker_environment(&fixture.credential.node_id, "user-1")
                .await
                .is_err()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("oversized carrier must unpublish the Worker");
    assert!(
        !fixture
            .control
            .nodes
            .get_node(
                &Actor::regular("user-1"),
                agent_protocol::GetNodeRequest {
                    node_id: fixture.credential.node_id
                }
            )
            .await
            .unwrap()
            .available
    );
    ingress.close().await;
}

#[tokio::test]
async fn grpc_register_honors_client_timeout_while_lifecycle_lock_is_held() {
    let fixture = Fixture::new(/* endpoint */ None).await;
    let ingress = Ingress::new(fixture.control.clone()).await;
    let _lifecycle = fixture
        .control
        .nodes
        .lock_lifecycle(&fixture.credential.node_id)
        .await;
    let mut request = fixture.registration(&["talos"]);
    request
        .metadata_mut()
        .insert("grpc-timeout", "200m".parse().unwrap());
    let started = std::time::Instant::now();
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        ingress.client.clone().register(request),
    )
    .await
    .expect("server should stop the blocked Register using the grpc-timeout")
    .expect_err("blocked Register should not succeed");
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(error.code(), tonic::Code::Cancelled);
    assert_eq!(error.message(), "Timeout expired");
    ingress.close().await;
}
