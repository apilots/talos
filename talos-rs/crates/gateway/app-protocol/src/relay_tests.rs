use super::*;

#[test]
fn registration_and_data_round_trip_without_losing_credentials_or_binary_content() {
    let encoded = serde_json::to_vec(&HostToRelay::Hello {
        version: RELAY_VERSION,
        host_id: "host".into(),
        auth: NodeAuth {
            node_id: "computer-a".into(),
            token: "test-lease".into(),
        },
    })
    .unwrap();
    let decoded: HostToRelay = serde_json::from_slice(&encoded).unwrap();
    decoded.validate().unwrap();
    let HostToRelay::Hello { version, auth, .. } = decoded else {
        panic!("expected Hello")
    };
    assert_eq!(
        (version, auth.node_id, auth.token),
        (RELAY_VERSION, "computer-a".into(), "test-lease".into())
    );
    let bytes = vec![0, 255, 4, 17];
    let encoded = serde_json::to_vec(&RelayToHost::Data {
        stream_id: 1,
        sequence: 1,
        data: bytes.clone(),
    })
    .unwrap();
    let decoded: RelayToHost = serde_json::from_slice(&encoded).unwrap();
    let RelayToHost::Data {
        stream_id,
        sequence,
        data,
    } = decoded
    else {
        panic!("expected Data")
    };
    assert_eq!((stream_id, sequence, data), (1, 1, bytes));
}
#[test]
fn credit_is_returned_only_for_data_consumed_in_order() {
    let mut sent = SendWindow::default();
    let mut received = ReceiveWindow::default();
    for seq in 1..=RELAY_WINDOW_FRAMES {
        assert_eq!(sent.reserve(), Some(seq));
        received.receive(seq).unwrap();
    }
    assert_eq!(sent.reserve(), None);
    assert!(received.receive(RELAY_WINDOW_FRAMES + 1).is_err());
    assert!(received.consume(2).is_err());
    let acknowledged = received.consume(1).unwrap();
    sent.acknowledge(acknowledged).unwrap();
    assert_eq!(sent.reserve(), Some(RELAY_WINDOW_FRAMES + 1));
    received.receive(RELAY_WINDOW_FRAMES + 1).unwrap();
    assert!(sent.acknowledge(RELAY_WINDOW_FRAMES + 2).is_err());
}
#[test]
fn malformed_targets_and_oversized_frames_fail_at_the_transport_boundary() {
    let mut head = RelayRequestHead {
        stream_id: 1,
        identity: RelayRequestIdentity(talos_auth::Actor::regular("owner")),
        method: "GET".into(),
        path_and_query: "/v1/session".into(),
        headers: vec![],
        lane: RelayLane::Interactive,
        callback_origin: None,
    };
    head.validate().unwrap();
    for invalid in [
        "https://another-host/private",
        "//another-host/private",
        "/private#fragment",
    ] {
        head.path_and_query = invalid.into();
        assert!(head.validate().is_err());
    }
    assert!(RelayToHost::Data {
        stream_id: 1,
        sequence: 1,
        data: vec![0; RELAY_CHUNK_BYTES + 1]
    }
    .validate()
    .is_err());
    let raw = serde_json::to_vec(&RelayToHost::Data {
        stream_id: 1,
        sequence: 1,
        data: vec![0; RELAY_CHUNK_BYTES + 4],
    })
    .unwrap();
    assert!(serde_json::from_slice::<RelayToHost>(&raw).is_err());
}

#[test]
fn delegated_identity_excludes_credentials_and_always_restores_remote_provenance() {
    let actor = talos_auth::Actor::regular("owner")
        .with_access_token("never-forward-token")
        .with_auth_disabled(true);
    let encoded = serde_json::to_string(&RelayRequestIdentity(actor)).unwrap();
    assert!(!encoded.contains("never-forward-token"));
    assert!(!encoded.contains("auth_disabled"));
    assert!(!encoded.contains("request_origin"));
    let decoded: RelayRequestIdentity = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.0.access_token.is_none());
    assert!(!decoded.0.auth_disabled);
    assert_eq!(
        decoded.0.request_origin,
        talos_auth::ProductRequestOrigin::RemoteProduct
    );
    assert!(serde_json::from_str::<RelayRequestIdentity>(r#"{"userId":"owner","username":null,"role":"user","userType":"free","requestOrigin":"LocalProduct"}"#).is_err());
}
