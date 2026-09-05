use super::*;

#[tokio::test]
async fn claimed_user_action_is_rebuilt_from_canonical_facts_after_restart() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let claim = claim_pending_action(store.as_ref()).await;

    service
        .recover_active_turn(test_session(), claim.active_turn)
        .await
        .expect("claimed response should resume from canonical facts");
    wait_for_canonical_terminal(store.as_ref()).await;

    let request = agent_execution
        .stream_request
        .lock()
        .unwrap()
        .clone()
        .expect("recovered continuation request should be captured");
    assert_eq!(
        (
            request.context.turn_id,
            request.input.message_id,
            request.checkpoint.is_some(),
            request.continuation.is_some(),
        ),
        (
            "turn-original".to_string(),
            "msg_action_action-1".to_string(),
            true,
            true,
        )
    );
}

#[tokio::test]
async fn claimed_continuation_with_progress_settles_without_reexecuting_provider() {
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, _) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let claim = claim_pending_action(store.as_ref()).await;
    let identity = claim.active_turn.identity.clone();
    let progress = store
        .commit_progress_event(
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: claim.events.last().unwrap().sequence + 1,
                occurred_at: 6,
                payload: CanonicalAgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
            },
            claim.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    service
        .recover_active_turn(test_session(), progress.active_turn)
        .await
        .unwrap();
    let terminal = store
        .get_latest_agent_terminal_event("s1")
        .await
        .unwrap()
        .unwrap();
    let active = store.get_agent_active_turn("s1").await.unwrap();
    assert_eq!(
        (
            execution.stream_requests.lock().unwrap().len(),
            terminal.identity,
            match terminal.payload {
                CanonicalAgentEventPayload::TurnFailed(failure) => Some(failure.error.code),
                _ => None,
            },
            active
        ),
        (
            0,
            identity,
            Some(talos_error::ErrorCode::ServiceUnavailable),
            None
        ),
    );
}

async fn claim_pending_action(
    store: &DatabaseSessionStore,
) -> talos_store::session::AgentEventCommit {
    insert_pending_dynamic_action(store, Some("tool-original")).await;
    let identity = talos_agent_api::AgentTurnRef::new("s1", "turn-original");
    let waiting = store
        .get_agent_active_turn("s1")
        .await
        .unwrap()
        .expect("waiting canonical Turn should exist");
    let events = store.list_agent_turn_events(&identity, 64).await.unwrap();
    let message = completed_user_action_message(
        &identity.turn_id,
        "msg_action_action-1",
        "action-1",
        UserActionResponse::DynamicToolResult(DynamicToolResult {
            output: "recovered".to_string(),
            success: true,
        }),
        5,
    )
    .unwrap();
    let admission = canonical_user_admission(
        identity.clone(),
        message,
        events.last().unwrap().sequence.saturating_add(1),
        false,
    );
    store
        .claim_continuation(talos_store::session::ClaimContinuationParams {
            events: admission,
            action_id: "action-1".to_string(),
            claim_id: "msg_action_action-1".to_string(),
            expected_event_seq: waiting.last_event_seq,
            command: None,
        })
        .await
        .expect("durable response claim should commit before restart")
}

#[tokio::test]
async fn running_turn_without_a_claimed_continuation_fails_after_restart() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let identity = talos_agent_api::AgentTurnRef::new("s1", "turn-orphaned");
    let input = completed_agent_user_input(
        &identity.turn_id,
        "message-orphaned",
        test_user_input("run"),
        1,
    )
    .unwrap();
    let admission = commit_test_running_turn(store.as_ref(), identity.clone(), input).await;
    message_queue
        .add("s1", test_user_input("queued after recovery"))
        .unwrap();

    service
        .recover_active_turn(test_session(), admission.active_turn)
        .await
        .expect("orphaned durable Turn should settle during restart recovery");

    wait_for_stream_requests(agent_execution.as_ref(), 1).await;
    let events = store.list_agent_turn_events(&identity, 32).await.unwrap();
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(CanonicalAgentEventPayload::TurnFailed(failed))
            if failed.error.code == talos_error::ErrorCode::ServiceUnavailable
    ));
}
