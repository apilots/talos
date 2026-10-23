use super::*;

async fn directory(a: &Peer, b: &Peer) {
    registered_computer(a).await;
    exchange(
        a,
        b,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
}
fn reservation(session: &str) -> CommandReservation {
    CommandReservation {
        session_id: session.into(),
        command: IdempotentCommand::TurnSubmission,
        idempotency_key: "original-key".into(),
        request_hash: "original-hash".into(),
        request_id: "original-request".into(),
        turn_id: "original-turn".into(),
        message_id: "original-message".into(),
        lease_id: "original-lease".into(),
        now: 10,
        lease_expires_at: 100,
        record_expires_at: Some(i64::MAX),
    }
}
#[tokio::test]
async fn cancellation_is_display_only_and_command_replay_retains_original_response() {
    let a = peer(true).await;
    let b = peer(false).await;
    directory(&a, &b).await;
    a.sessions
        .insert_session_record(&record("session-cancel"))
        .await
        .unwrap();
    // Another local timeline creates different DB cursors on B.
    let mut unrelated = record("unrelated");
    unrelated.execution_node_id = COMPUTER.into();
    b.sessions.insert_session_record(&unrelated).await.unwrap();
    run(&b, "unrelated", "unrelated-turn").await;
    let resource = SyncResourceKey::Session {
        session_id: "session-cancel".into(),
    };
    exchange(&a, &b, resource.clone()).await;
    let request = reservation("session-cancel");
    let IdempotencyReservation::Reserved(lease) =
        a.sessions.reserve_command(request.clone()).await.unwrap()
    else {
        panic!("fresh command must reserve")
    };
    // A reservation has no new shared execution fact.
    assert!(a
        .sync
        .pending_resources(a.adapter.binding(), 10)
        .await
        .unwrap()
        .is_empty());
    let identity = AgentTurnRef::new("session-cancel", "original-turn");
    let initial = a
        .sessions
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: vec![
                event(
                    "session-cancel",
                    "original-turn",
                    1,
                    AgentEventPayload::TurnStarted(AgentTurnStarted {}),
                ),
                event(
                    "session-cancel",
                    "original-turn",
                    2,
                    AgentEventPayload::MessageCompleted(AgentMessage {
                        message_id: "original-message".into(),
                        turn_id: "original-turn".into(),
                        role: AgentMessageRole::User,
                        status: AgentMessageStatus::Completed,
                        blocks: vec![],
                        created_at: 10,
                        completed_at: Some(10),
                    }),
                ),
            ],
            command: Some(lease),
        })
        .await
        .unwrap();
    exchange(&a, &b, resource.clone()).await;
    assert!(b
        .sessions
        .get_agent_active_turn("session-cancel")
        .await
        .unwrap()
        .is_none());
    let cancelling = a.sessions.begin_cancellation(&identity).await.unwrap();
    exchange(&a, &b, resource.clone()).await;
    let display = b
        .sessions
        .get_shared_active_turn("session-cancel")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(display.state, ActiveTurnState::Cancelling);
    assert_eq!(
        display.initial_message_id.as_deref(),
        Some("original-message")
    );
    assert_ne!(
        display.last_event_id,
        TimelineEventId::from_seq(initial.active_turn.last_event_seq).to_string()
    );
    a.sessions.restore_cancellation(cancelling).await.unwrap();
    exchange(&a, &b, resource.clone()).await;
    assert_eq!(
        b.sessions
            .get_shared_active_turn("session-cancel")
            .await
            .unwrap()
            .unwrap()
            .state,
        ActiveTurnState::Running
    );
    let IdempotencyReservation::Completed(response) =
        b.sessions.reserve_command(request.clone()).await.unwrap()
    else {
        panic!("imported command must replay")
    };
    assert_eq!(response.request_id, request.request_id);
    assert_eq!(response.turn_id, request.turn_id);
    assert_eq!(
        response.message_id.as_deref(),
        Some(request.message_id.as_str())
    );
    assert_eq!(response.outcome, Some(TurnCommandStatus::Accepted));
    assert_eq!(
        response.committed_event_seq,
        Some(
            TimelineEventId::from(display.last_event_id.clone())
                .event_seq()
                .unwrap()
        )
    );
    assert!(b
        .sessions
        .get_agent_active_turn("session-cancel")
        .await
        .unwrap()
        .is_none());
    let terminal = a
        .sessions
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                "session-cancel",
                "original-turn",
                3,
                AgentEventPayload::TurnFailed(AgentTurnFailed {
                    error: talos_error::ApiError::with_message(
                        talos_error::ErrorCode::Cancelled,
                        "Cancelled",
                    ),
                    retryable: false,
                }),
            ),
            expected_event_seq: initial.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    a.sessions
        .advance_memory_processed_until_event_seq("session-cancel", 0, terminal.event_seq as u64)
        .await
        .unwrap();
    exchange(&a, &b, resource).await;
    assert!(b
        .sessions
        .get_shared_active_turn("session-cancel")
        .await
        .unwrap()
        .is_none());
    let loaded = b
        .sessions
        .get_session_record("session-cancel")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        loaded.memory_processed_until_event_seq,
        terminal.event_seq as u64
    );
    assert_eq!(
        loaded.memory_processed_until_event_seq as i64,
        b.sessions
            .get_latest_timeline_event_seq("session-cancel")
            .await
            .unwrap()
            .unwrap()
    );
    let bound_worker = b
        .sessions
        .get_shared_session_info("session-cancel")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound_worker.model_selection.model_ref.id, "worker-model");
    assert_eq!(
        bound_worker.configured_default_mode,
        UserPermissionMode::AskForApproval
    );
}

#[tokio::test]
async fn enabling_capture_preserves_a_large_existing_history_at_the_pre_mutation_cut() {
    let a = peer(true).await;
    let b = peer(false).await;
    directory(&a, &b).await;
    let local = DatabaseSessionStore::new(a.storage.clone()).await.unwrap();
    local
        .insert_session_record(&record("large-history"))
        .await
        .unwrap();
    let admitted = local
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: vec![
                event(
                    "large-history",
                    "turn",
                    1,
                    AgentEventPayload::TurnStarted(AgentTurnStarted {}),
                ),
                event(
                    "large-history",
                    "turn",
                    2,
                    AgentEventPayload::MessageCompleted(AgentMessage {
                        message_id: "message".into(),
                        turn_id: "turn".into(),
                        role: AgentMessageRole::User,
                        status: AgentMessageStatus::Completed,
                        blocks: vec![],
                        created_at: 1,
                        completed_at: Some(1),
                    }),
                ),
            ],
            command: None,
        })
        .await
        .unwrap();
    local
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                "large-history",
                "turn",
                3,
                AgentEventPayload::TurnFailed(AgentTurnFailed {
                    error: talos_error::ApiError::with_message(
                        talos_error::ErrorCode::Cancelled,
                        "Cancelled",
                    ),
                    retryable: false,
                }),
            ),
            expected_event_seq: admitted.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    for n in 0..270 {
        local
            .append_timeline_notice(TimelineSystemNotice {
                identity: AgentTurnRef::new("large-history", "turn"),
                occurred_at: n,
                notice: AgentSystemNotice {
                    level: TalosTimelineNoticeLevel::Info,
                    content: SystemNoticeContent::Text(format!(
                        "notice-{n}:{}",
                        "x".repeat(33_000)
                    )),
                },
            })
            .await
            .unwrap();
    }
    // First captured mutation must establish all baseline pages before this patch.
    a.sessions
        .patch_session(PatchSessionParams {
            session_id: "large-history".into(),
            conf: SessionConf {
                model_selection: None,
                user_permission_mode: Some(UserPermissionMode::FullAccess),
            },
            current_directory: None,
            last_activity: 500,
        })
        .await
        .unwrap();
    exchange(
        &a,
        &b,
        SyncResourceKey::Session {
            session_id: "large-history".into(),
        },
    )
    .await;
    let facts = b
        .sessions
        .list_timeline_events_after("large-history", 0, 1_000)
        .await
        .unwrap();
    assert_eq!(facts.len(), 273);
    assert_eq!(
        b.sessions
            .get_session_record("large-history")
            .await
            .unwrap()
            .unwrap()
            .conf
            .user_permission_mode,
        Some(UserPermissionMode::FullAccess)
    );
    assert!(b
        .sync
        .pending_resources(b.adapter.binding(), 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn explicit_local_claim_preserves_worker_namespace_and_other_accounts() {
    let peer = peer(true).await;
    let mut local = record("anonymous-history");
    local.user_id = "anonymous".into();
    peer.sessions.insert_session_record(&local).await.unwrap();
    let mut unrelated = record("other-account-history");
    unrelated.user_id = "other-account".into();
    peer.sessions
        .insert_session_record(&unrelated)
        .await
        .unwrap();
    peer.sessions
        .claim_local_history("account-1")
        .await
        .unwrap();
    assert_eq!(
        peer.sessions
            .get_session_record("anonymous-history")
            .await
            .unwrap()
            .unwrap()
            .user_id,
        "account-1"
    );
    assert_eq!(
        peer.sessions
            .get_workspace_user_id("anonymous-history")
            .await
            .unwrap()
            .as_deref(),
        Some("anonymous")
    );
    assert_eq!(
        peer.sessions
            .get_session_record("other-account-history")
            .await
            .unwrap()
            .unwrap()
            .user_id,
        "other-account"
    );
}
