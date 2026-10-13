//! Controlled TLS ingress regression, using the real directory and publication owner.
use super::*;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, ClientTlsConfig};

struct Ingress {
    client: pb::agent_node_control_client::AgentNodeControlClient<Channel>,
    endpoint: String,
    trust: agent_client::AgentTlsTrust,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl Ingress {
    async fn new(control: NodeControlRpc) -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let pem = certificate.cert.pem();
        let identity = Identity::from_pem(&pem, certificate.signing_key.serialize_pem());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let incoming = futures_util::stream::unfold(listener, |listener| async {
            let result = listener.accept().await.map(|(stream, _)| stream);
            Some((result, listener))
        });
        let stop = control.shutdown.clone();
        let server_stop = stop.clone();
        let task = tokio::spawn(async move {
            Server::builder()
                .tls_config(ServerTlsConfig::new().identity(identity))
                .unwrap()
                .add_service(pb::agent_node_control_server::AgentNodeControlServer::new(
                    control,
                ))
                .serve_with_incoming_shutdown(incoming, server_stop.cancelled_owned())
                .await
                .unwrap();
        });
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
            stop,
            task,
        }
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
            }
        ))
    ));
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
        let _lifecycle = fixture.publisher.nodes.lock_lifecycle().await;
        fixture
            .publisher
            .coordinator
            .ensure_agent("talos")
            .await
            .unwrap();
        assert!(
            fixture
                .publisher
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
        .publisher
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

#[path = "worker_environment_rpc_tests.rs"]
mod environment_tests;
