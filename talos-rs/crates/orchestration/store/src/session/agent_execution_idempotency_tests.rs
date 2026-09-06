use super::*;

fn reservation(
    request_hash: &str,
    request_id: &str,
    turn_id: &str,
    message_id: &str,
    lease_id: &str,
    now: i64,
) -> CommandReservation {
    CommandReservation {
        session_id: "session-1".to_string(),
        command: IdempotentCommand::TurnSubmission,
        idempotency_key: "command-1".to_string(),
        request_hash: request_hash.to_string(),
        request_id: request_id.to_string(),
        turn_id: turn_id.to_string(),
        message_id: message_id.to_string(),
        lease_id: lease_id.to_string(),
        now,
        lease_expires_at: now + 10,
        record_expires_at: Some(now + 100),
    }
}

fn expect_lease(reservation: IdempotencyReservation) -> CommandLease {
    let IdempotencyReservation::Reserved(lease) = reservation else {
        panic!("expected a new idempotency lease");
    };
    lease
}

#[tokio::test]
async fn turn_admission_atomically_completes_its_idempotency_record() {
    let store = store().await;
    let now = chrono::Utc::now().timestamp_millis();
    let lease = expect_lease(
        store
            .reserve_command(reservation(
                "hash-1",
                "request-1",
                "turn-1",
                "message-1",
                "lease-1",
                now,
            ))
            .await
            .unwrap(),
    );

    let commit = store
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: admission(),
            command: Some(lease),
        })
        .await
        .unwrap();
    let replay = store
        .reserve_command(reservation(
            "hash-1",
            "request-unused",
            "turn-unused",
            "message-unused",
            "lease-2",
            now.saturating_add(1),
        ))
        .await
        .unwrap();
    let IdempotencyReservation::Completed(record) = replay else {
        panic!("committed admission must replay its normalized result");
    };

    assert_eq!(
        (
            record.request_id.as_str(),
            record.turn_id.as_str(),
            record.message_id.as_deref(),
            record.outcome,
            record.committed_event_seq,
        ),
        (
            "request-1",
            "turn-1",
            Some("message-1"),
            Some(talos_types::TurnCommandStatus::Accepted),
            Some(commit.events.last().unwrap().event_seq),
        )
    );
    assert!(record.updated_at >= record.created_at);
}

#[tokio::test]
async fn expired_completed_record_releases_its_key_for_a_new_command() {
    let store = store().await;
    let lease = expect_lease(
        store
            .reserve_command(reservation(
                "hash-1",
                "request-1",
                "turn-1",
                "message-1",
                "lease-1",
                10,
            ))
            .await
            .unwrap(),
    );
    store
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: admission(),
            command: Some(lease),
        })
        .await
        .unwrap();

    let next = expect_lease(
        store
            .reserve_command(reservation(
                "hash-2",
                "request-2",
                "turn-2",
                "message-2",
                "lease-2",
                111,
            ))
            .await
            .unwrap(),
    );

    assert_eq!(
        (
            next.request_id.as_str(),
            next.turn_id.as_str(),
            next.message_id.as_str(),
        ),
        ("request-2", "turn-2", "message-2")
    );
}

#[tokio::test]
async fn unexpired_reservation_reports_in_progress() {
    let store = store().await;
    store
        .reserve_command(reservation(
            "hash-1",
            "request-1",
            "turn-1",
            "message-1",
            "lease-1",
            10,
        ))
        .await
        .unwrap();

    let replay = store
        .reserve_command(reservation(
            "hash-1",
            "request-2",
            "turn-2",
            "message-2",
            "lease-2",
            15,
        ))
        .await
        .unwrap();
    assert!(matches!(replay, IdempotencyReservation::InProgress));
}

#[tokio::test]
async fn expired_reservation_without_a_fact_reclaims_the_existing_identity() {
    let store = store().await;
    store
        .reserve_command(reservation(
            "hash-1",
            "request-1",
            "turn-1",
            "message-1",
            "lease-1",
            10,
        ))
        .await
        .unwrap();

    let lease = expect_lease(
        store
            .reserve_command(reservation(
                "hash-1",
                "request-2",
                "turn-2",
                "message-2",
                "lease-2",
                21,
            ))
            .await
            .unwrap(),
    );

    assert_eq!(
        (
            lease.request_id.as_str(),
            lease.turn_id.as_str(),
            lease.message_id.as_str(),
            lease.lease_id.as_str(),
        ),
        ("request-1", "turn-1", "message-1", "lease-2")
    );
}

#[tokio::test]
async fn expired_reservation_repairs_an_existing_canonical_admission() {
    let store = store().await;
    store
        .reserve_command(reservation(
            "hash-1",
            "request-1",
            "turn-1",
            "message-1",
            "lease-1",
            10,
        ))
        .await
        .unwrap();
    let commit = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();

    let repaired = store
        .reserve_command(reservation(
            "hash-1",
            "request-2",
            "turn-2",
            "message-2",
            "lease-2",
            21,
        ))
        .await
        .unwrap();
    let IdempotencyReservation::Completed(record) = repaired else {
        panic!("canonical admission must repair the expired reservation");
    };

    assert_eq!(
        (record.outcome, record.committed_event_seq),
        (
            Some(talos_types::TurnCommandStatus::Accepted),
            Some(commit.events[0].event_seq),
        )
    );
}

#[tokio::test]
async fn conflicting_request_hash_never_reuses_the_same_key() {
    let store = store().await;
    store
        .reserve_command(reservation(
            "hash-1",
            "request-1",
            "turn-1",
            "message-1",
            "lease-1",
            10,
        ))
        .await
        .unwrap();

    let error = store
        .reserve_command(reservation(
            "hash-2",
            "request-2",
            "turn-2",
            "message-2",
            "lease-2",
            21,
        ))
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::IdempotencyConflict));
}

#[tokio::test]
async fn pre_admission_failure_releases_only_its_exact_lease() {
    let store = store().await;
    let lease = expect_lease(
        store
            .reserve_command(reservation(
                "hash-1",
                "request-1",
                "turn-1",
                "message-1",
                "lease-1",
                10,
            ))
            .await
            .unwrap(),
    );
    store.release_command(lease).await.unwrap();

    let next = store
        .reserve_command(reservation(
            "hash-2",
            "request-2",
            "turn-2",
            "message-2",
            "lease-2",
            11,
        ))
        .await
        .unwrap();

    assert!(matches!(next, IdempotencyReservation::Reserved(_)));
}
