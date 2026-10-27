use super::*;
use std::time::Duration;
use talos_types::{BrowserInputMode, DesktopStreamBinding};

struct MediaFixture {
    client: AgentNodeClient,
    endpoint: String,
    trust: agent_client::AgentTlsTrust,
    _server: tokio_util::task::AbortOnDropHandle<()>,
}

impl MediaFixture {
    async fn new(fixture: &Fixture) -> Self {
        let media = super::super::super::desktop::DesktopMediaRpc::new(
            fixture.management.clone(),
            CancellationToken::new(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let endpoint = format!("https://{address}");
        let trust = agent_client::AgentTlsTrust::Certificate(certificate.cert.pem().into_bytes());
        let client = AgentNodeClient::connect(&endpoint, trust.clone())
            .await
            .unwrap();
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            tonic::transport::Server::builder()
                .tls_config(tonic::transport::ServerTlsConfig::new().identity(
                    tonic::transport::Identity::from_pem(
                        certificate.cert.pem(),
                        certificate.signing_key.serialize_pem(),
                    ),
                ))
                .unwrap()
                .add_service(pb::agent_desktop_media_server::AgentDesktopMediaServer::new(media))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        }));
        Self {
            client,
            endpoint,
            trust,
            _server: server,
        }
    }
}

#[tokio::test]
async fn failed_desktop_admission_cancellation_releases_only_its_owned_reservation() {
    let fixture = Fixture::new(None).await;
    let media = MediaFixture::new(&fixture).await;
    let client = &media.client;
    let node = fixture.credential.node_id;
    let viewer_id = uuid::Uuid::new_v4().to_string();
    let generation = uuid::Uuid::new_v4().to_string();
    let binding = || DesktopStreamBinding {
        stream_id: uuid::Uuid::new_v4().to_string(),
        target_id: "managed".into(),
        viewer_id: viewer_id.clone(),
        generation: generation.clone(),
        input_mode: BrowserInputMode::ReadOnly,
        controller_id: None,
        input_epoch: None,
    };
    let original = binding();
    let original_reservation = client
        .reserve_desktop("user-1", node.clone(), original.clone())
        .await
        .unwrap();
    for _ in 0..3 {
        let candidate = binding();
        let reservation = client
            .reserve_desktop("user-1", node.clone(), candidate.clone())
            .await
            .unwrap();
        assert!(client
            .reserve_desktop("user-1", node.clone(), binding())
            .await
            .err()
            .expect("capacity should reject a third candidate")
            .is_code(ErrorCode::Conflict));
        for (token, ticket, code) in [
            (
                "user-2",
                reservation.ticket.clone(),
                ErrorCode::ResourceNotFound,
            ),
            (
                "user-1",
                uuid::Uuid::new_v4().to_string(),
                ErrorCode::PermissionDenied,
            ),
        ] {
            assert!(client
                .cancel_desktop(token, node.clone(), candidate.stream_id.clone(), ticket)
                .await
                .unwrap_err()
                .is_code(code));
        }
        client
            .cancel_desktop(
                "user-1",
                node.clone(),
                candidate.stream_id.clone(),
                reservation.ticket.clone(),
            )
            .await
            .unwrap();
        assert!(client
            .cancel_desktop(
                "user-1",
                node.clone(),
                candidate.stream_id,
                reservation.ticket
            )
            .await
            .unwrap_err()
            .is_code(ErrorCode::PermissionDenied));
    }
    // Candidate cleanup must leave the original viewer reservation intact.
    client
        .cancel_desktop(
            "user-1",
            node,
            original.stream_id,
            original_reservation.ticket,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn desktop_rpc_relay_carries_both_directions_and_rechecks_the_remote_lease() {
    let fixture = Fixture::new(None).await;
    let lease = registered(
        fixture
            .control
            .register(fixture.registration(&["talos"]))
            .await
            .unwrap()
            .into_inner(),
    );
    let media = MediaFixture::new(&fixture).await;
    let binding = DesktopStreamBinding {
        stream_id: uuid::Uuid::new_v4().to_string(),
        viewer_id: uuid::Uuid::new_v4().to_string(),
        generation: uuid::Uuid::new_v4().to_string(),
        target_id: "managed".into(),
        input_mode: BrowserInputMode::ReadOnly,
        controller_id: None,
        input_epoch: None,
    };
    let node = fixture.credential.node_id.clone();
    let reservation = media
        .client
        .reserve_desktop("user-1", node.clone(), binding.clone())
        .await
        .unwrap();
    let request = talos_types::WorkerDesktopOpen {
        binding: binding.clone(),
        nonce: reservation.nonce,
    };
    let unleased = AgentSessionDataClient::connect(
        &media.endpoint,
        media.trust.clone(),
        agent_protocol::NodeAuth {
            node_id: node.clone(),
            token: fixture.credential.token.clone(),
        },
    )
    .await
    .unwrap();
    assert!(unleased
        .open_desktop_media(request.clone())
        .await
        .err()
        .unwrap()
        .is_code(ErrorCode::Unauthenticated));
    let resources = AgentSessionDataClient::connect(
        &media.endpoint,
        media.trust.clone(),
        agent_protocol::NodeAuth {
            node_id: node.clone(),
            token: lease,
        },
    )
    .await
    .unwrap();
    let mut worker = resources.open_desktop_media(request).await.unwrap();
    assert!(media
        .client
        .view_desktop(
            "user-2",
            node.clone(),
            binding.stream_id.clone(),
            reservation.ticket.clone()
        )
        .await
        .is_err());
    let mut viewer = media
        .client
        .view_desktop(
            "user-1",
            node.clone(),
            binding.stream_id,
            reservation.ticket,
        )
        .await
        .unwrap();
    worker
        .outgoing
        .send(b"display-through-tls".to_vec())
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), viewer.incoming.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        b"display-through-tls"
    );
    viewer
        .outgoing
        .send(b"input-through-tls".to_vec())
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), worker.incoming.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        b"input-through-tls"
    );
    fixture
        .management
        .update_node(management_request(
            "user-1",
            pb::UpdateNodeRequest {
                node_id: node,
                display_name: "Revoked computer".into(),
                endpoint: None,
                status: pb::NodeStatus::Disabled as i32,
            },
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), worker.stopped.cancelled())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), viewer.stopped.cancelled())
        .await
        .unwrap();
}
