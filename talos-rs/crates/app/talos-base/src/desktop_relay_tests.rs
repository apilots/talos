use super::*;

fn binding(viewer: &str) -> DesktopStreamBinding {
    DesktopStreamBinding {
        stream_id: uuid::Uuid::new_v4().to_string(),
        viewer_id: viewer.into(),
        generation: uuid::Uuid::new_v4().to_string(),
        target_id: "managed".into(),
        input_mode: BrowserInputMode::ReadOnly,
        controller_id: None,
        input_epoch: None,
    }
}

async fn pair(relay: &DesktopRelay) -> (DesktopTransport, DesktopTransport) {
    let binding = binding(&uuid::Uuid::new_v4().to_string());
    let reservation = relay
        .reserve("alice".into(), "embedded".into(), binding.clone())
        .await
        .unwrap();
    let worker = relay
        .attach_local_worker(WorkerDesktopOpen {
            binding: binding.clone(),
            nonce: reservation.nonce,
        })
        .await
        .unwrap();
    let viewer = relay
        .viewer("alice", "embedded", &binding.stream_id, &reservation.ticket)
        .await
        .unwrap();
    (worker, viewer)
}

#[tokio::test]
async fn relay_preserves_identity_binding_single_use_and_bidirectional_bytes() {
    let relay = DesktopRelay::new(CancellationToken::new());
    let binding = binding(&uuid::Uuid::new_v4().to_string());
    let reservation = relay
        .reserve("alice".into(), "embedded".into(), binding.clone())
        .await
        .unwrap();
    assert!(relay.contains_reservation(&binding.stream_id).await);
    let request = WorkerDesktopOpen {
        binding: binding.clone(),
        nonce: reservation.nonce.clone(),
    };
    for (owner, node) in [("bob", "embedded"), ("alice", "another-node")] {
        assert!(relay
            .worker(owner, node, request.clone())
            .await
            .err()
            .unwrap()
            .is_code(ErrorCode::PermissionDenied));
    }
    let mut changed = request.clone();
    changed.binding.generation = uuid::Uuid::new_v4().to_string();
    assert!(relay.attach_local_worker(changed).await.is_err());
    let mut changed = request.clone();
    changed.nonce = uuid::Uuid::new_v4().to_string();
    assert!(relay.attach_local_worker(changed).await.is_err());
    let mut worker = relay.attach_local_worker(request.clone()).await.unwrap();
    assert!(relay.attach_local_worker(request).await.is_err());
    for (owner, node, ticket) in [
        ("bob", "embedded", reservation.ticket.clone()),
        ("alice", "another-node", reservation.ticket.clone()),
        ("alice", "embedded", uuid::Uuid::new_v4().to_string()),
    ] {
        assert!(relay
            .viewer(owner, node, &binding.stream_id, &ticket)
            .await
            .is_err());
    }
    let mut viewer = relay
        .viewer("alice", "embedded", &binding.stream_id, &reservation.ticket)
        .await
        .unwrap();
    assert!(relay
        .viewer("alice", "embedded", &binding.stream_id, &reservation.ticket)
        .await
        .is_err());
    let display = vec![17; 64 * 1024];
    worker.outgoing.send(display.clone()).await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), viewer.incoming.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        display
    );
    viewer
        .outgoing
        .send(b"viewer-input".to_vec())
        .await
        .unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), worker.incoming.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        b"viewer-input"
    );
    drop(viewer);
    timeout(Duration::from_secs(1), worker.stopped.cancelled())
        .await
        .unwrap();
}

#[tokio::test]
async fn failed_candidate_isolated_until_worker_admission_replaces_the_current_view() {
    let relay = DesktopRelay::new(CancellationToken::new());
    let viewer_id = uuid::Uuid::new_v4().to_string();
    let original = binding(&viewer_id);
    let reservation = relay
        .reserve("alice".into(), "embedded".into(), original.clone())
        .await
        .unwrap();
    let original_worker = relay
        .attach_local_worker(WorkerDesktopOpen {
            binding: original.clone(),
            nonce: reservation.nonce,
        })
        .await
        .unwrap();
    let original_viewer = relay
        .viewer(
            "alice",
            "embedded",
            &original.stream_id,
            &reservation.ticket,
        )
        .await
        .unwrap();
    let candidate = binding(&viewer_id);
    let candidate_ticket = relay
        .reserve("alice".into(), "embedded".into(), candidate.clone())
        .await
        .unwrap();
    assert!(relay
        .reserve("alice".into(), "embedded".into(), binding(&viewer_id))
        .await
        .err()
        .unwrap()
        .is_code(ErrorCode::Conflict));
    assert!(relay
        .attach_local_worker(WorkerDesktopOpen {
            binding: candidate.clone(),
            nonce: uuid::Uuid::new_v4().to_string(),
        })
        .await
        .is_err());
    for (owner, ticket) in [
        ("bob", candidate_ticket.ticket.clone()),
        ("alice", uuid::Uuid::new_v4().to_string()),
    ] {
        assert!(relay
            .cancel(owner, "embedded", &candidate.stream_id, &ticket)
            .await
            .is_err());
    }
    assert!(!original_worker.stopped.is_cancelled());
    relay
        .cancel(
            "alice",
            "embedded",
            &candidate.stream_id,
            &candidate_ticket.ticket,
        )
        .await
        .unwrap();
    assert!(!relay.contains_reservation(&candidate.stream_id).await);
    assert!(!original_viewer.stopped.is_cancelled());
    let replacement = binding(&viewer_id);
    let ticket = relay
        .reserve("alice".into(), "embedded".into(), replacement.clone())
        .await
        .unwrap();
    let replacement_worker = relay
        .attach_local_worker(WorkerDesktopOpen {
            binding: replacement,
            nonce: ticket.nonce,
        })
        .await
        .unwrap();
    assert!(original_worker.stopped.is_cancelled());
    assert!(!replacement_worker.stopped.is_cancelled());
}

#[tokio::test]
async fn expired_reservations_and_stale_cleanup_cannot_remove_a_reused_stream() {
    let relay = DesktopRelay::new(CancellationToken::new());
    let binding = binding(&uuid::Uuid::new_v4().to_string());
    let reservation = relay
        .reserve("alice".into(), "embedded".into(), binding.clone())
        .await
        .unwrap();
    {
        let mut entries = relay.reservations.lock().await;
        Arc::get_mut(entries.get_mut(&binding.stream_id).unwrap())
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
    }
    assert!(relay
        .attach_local_worker(WorkerDesktopOpen {
            binding: binding.clone(),
            nonce: reservation.nonce
        })
        .await
        .is_err());
    let stale = relay
        .reservations
        .lock()
        .await
        .get(&binding.stream_id)
        .unwrap()
        .clone();
    let fresh = relay
        .reserve("alice".into(), "embedded".into(), binding.clone())
        .await
        .unwrap();
    assert!(stale.stopped.is_cancelled());
    relay.remove(&stale).await;
    assert!(relay.contains_reservation(&binding.stream_id).await);
    relay
        .cancel("alice", "embedded", &binding.stream_id, &fresh.ticket)
        .await
        .unwrap();
}

#[tokio::test]
async fn invalid_frames_slow_consumers_and_shutdown_close_the_shared_connection() {
    for data in [vec![], vec![0; 64 * 1024 + 1]] {
        let relay = DesktopRelay::new(CancellationToken::new());
        let (worker, viewer) = pair(&relay).await;
        viewer.outgoing.send(data).await.unwrap();
        timeout(Duration::from_secs(1), worker.stopped.cancelled())
            .await
            .unwrap();
    }
    let relay = DesktopRelay::new(CancellationToken::new());
    let (worker, viewer) = pair(&relay).await;
    // Never read viewer.incoming: the bounded display queue must stop this producer.
    timeout(Duration::from_secs(4), async {
        while worker.outgoing.send(vec![1]).await.is_ok() {}
    })
    .await
    .unwrap();
    assert!(viewer.stopped.is_cancelled());
    let shutdown = CancellationToken::new();
    let relay = DesktopRelay::new(shutdown.clone());
    let (worker, viewer) = pair(&relay).await;
    shutdown.cancel();
    timeout(Duration::from_secs(1), worker.stopped.cancelled())
        .await
        .unwrap();
    assert!(viewer.stopped.is_cancelled());
}
