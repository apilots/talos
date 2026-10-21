use super::*;
use talos_types::{BrowserInputMode, DesktopStreamBinding};

#[tokio::test]
async fn failed_desktop_admission_cancellation_releases_only_its_owned_reservation() {
    let fixture = Fixture::new(None).await;
    let media = super::super::super::desktop::DesktopMediaRpc::new(
        fixture.management.clone(),
        CancellationToken::new(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let client = AgentNodeClient::connect(
        &format!("https://{address}"),
        agent_client::AgentTlsTrust::Certificate(certificate.cert.pem().into_bytes()),
    )
    .await
    .unwrap();
    let _server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
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
