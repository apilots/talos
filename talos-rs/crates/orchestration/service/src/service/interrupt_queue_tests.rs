use super::*;

#[tokio::test]
async fn interrupt_requires_the_current_turn_and_pauses_until_explicit_queue_start() {
    let cancelled_turn_ids = Arc::new(Mutex::new(Vec::new()));
    let (service, store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::Cancelled,
            cancelled_turn_ids: cancelled_turn_ids.clone(),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-queue", request_source(), None),
        )
        .await
        .expect("session should be created");
    let identity = talos_agent_api::AgentTurnRef::new("session-interrupt-queue", "turn-current");
    let message = crate::turn::completed_agent_user_input(
        &identity.turn_id,
        "msg-current",
        crate::test_support::test_user_input("running"),
        1,
    )
    .expect("canonical User message should be valid");
    crate::test_support::commit_test_running_turn(store.as_ref(), identity.clone(), message).await;

    let stale_error = service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new("session-interrupt-queue", "turn-stale"),
            },
        )
        .await
        .expect_err("stale interrupt must not cancel the current turn");
    assert!(stale_error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert!(cancelled_turn_ids.lock().unwrap().is_empty());

    service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new("session-interrupt-queue", "turn-current"),
            },
        )
        .await
        .expect("current turn interrupt should be accepted");
    assert_eq!(
        *cancelled_turn_ids.lock().unwrap(),
        vec!["turn-current".to_string()]
    );
    assert!(store
        .get_agent_active_turn("session-interrupt-queue")
        .await
        .expect("active turn should load after interrupt")
        .is_none());
    let queue = service
        .message_queue
        .add(
            "session-interrupt-queue",
            crate::test_support::test_user_input("continue explicitly"),
        )
        .unwrap();
    assert!(service
        .turns
        .try_dispatch_next("session-interrupt-queue")
        .await
        .unwrap()
        .is_none());

    let session = service
        .access
        .load_session(&actor, "session-interrupt-queue")
        .await
        .expect("session should be readable");
    service
        .turns
        .start_queued_message(session, Some(&queue.messages[0].id))
        .await
        .expect("explicit start should resume queue execution");

    assert!(service
        .message_queue
        .list("session-interrupt-queue")
        .unwrap()
        .messages
        .is_empty());
}

#[tokio::test]
async fn interrupt_restores_active_turn_when_harness_rejects_interruption() {
    let (service, store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::Rejected,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-rejected", request_source(), None),
        )
        .await
        .expect("session should be created");
    commit_running_turn(
        store.as_ref(),
        "session-interrupt-rejected",
        "turn-existing",
        "msg-existing",
    )
    .await;

    let error = service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new(
                    "session-interrupt-rejected",
                    "turn-existing",
                ),
            },
        )
        .await
        .expect_err("Harness rejection should be returned");

    assert!(error
        .diagnostic_message()
        .contains("Harness rejected interruption"));
    let active_turn = store
        .get_agent_active_turn("session-interrupt-rejected")
        .await
        .expect("active turn should load")
        .expect("active turn should remain available");
    assert_eq!(active_turn.state, talos_agent_api::ActiveTurnState::Running);
}

#[tokio::test]
async fn unconfirmed_cleanup_survives_interrupt_and_timeline_reconnection() {
    let (service, store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::CleanupUnconfirmed,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-cleanup-unconfirmed", request_source(), None),
        )
        .await
        .unwrap();
    commit_running_turn(
        store.as_ref(),
        "session-cleanup-unconfirmed",
        "turn-existing",
        "msg-existing",
    )
    .await;

    let response = service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new(
                    "session-cleanup-unconfirmed",
                    "turn-existing",
                ),
            },
        )
        .await
        .expect("unconfirmed cleanup should be returned as a structured outcome");

    assert_eq!(
        response.outcome,
        talos_agent_api::AgentInterruptOutcome::CleanupUnconfirmed { task_count: 1 }
    );
    let identity = talos_types::AgentTurnRef::new("session-cleanup-unconfirmed", "turn-existing");
    let before = store.list_agent_turn_events(&identity, 32).await.unwrap();
    let recovery_error = service
        .recover_timeline(
            &actor,
            talos_types::RecoverTimelineRequest {
                session_id: identity.session_id.clone(),
                after_event_id: None,
                replay_limit: Some(32),
            },
        )
        .await
        .unwrap_err();
    assert!(recovery_error.is_code(talos_error::ErrorCode::RunCleanupUnconfirmed));
    let subscription = service
        .open_timeline_subscription(
            &actor,
            talos_types::SubscribeTimelineRequest {
                session_id: identity.session_id.clone(),
                turn_id: Some(identity.turn_id.clone()),
                since_event_id: None,
                replay_limit: Some(32),
                close_when_idle: Some(true),
                close_on_terminal: Some(true),
            },
        )
        .await;
    assert!(
        matches!(subscription, Err(error) if error.is_code(talos_error::ErrorCode::RunCleanupUnconfirmed))
    );
    assert_eq!(
        store.list_agent_turn_events(&identity, 32).await.unwrap(),
        before
    );
    assert_eq!(
        store
            .get_agent_active_turn("session-cleanup-unconfirmed")
            .await
            .unwrap()
            .unwrap()
            .state,
        talos_agent_api::ActiveTurnState::Cancelling
    );
}

#[tokio::test]
async fn interrupting_a_waiting_action_commits_one_canonical_terminal_fact() {
    use talos_types::{
        DynamicToolResultParams, PendingUserAction, UserActionParams, UserActionState,
    };

    let cancelled_turn_ids = Arc::new(Mutex::new(Vec::new()));
    let (service, store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::MissingLiveTurn,
            cancelled_turn_ids: cancelled_turn_ids.clone(),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-waiting-cancel", request_source(), None),
        )
        .await
        .unwrap();
    let identity = talos_agent_api::AgentTurnRef::new("session-waiting-cancel", "turn-waiting");
    let mut admission = commit_running_turn(
        store.as_ref(),
        &identity.session_id,
        &identity.turn_id,
        "msg-waiting",
    )
    .await;
    admission = store
        .commit_progress_event(
            talos_agent_api::AgentEvent {
                identity: identity.clone(),
                sequence: admission.events.last().unwrap().sequence.saturating_add(1),
                occurred_at: 2,
                payload: talos_agent_api::AgentEventPayload::MessageStarted(
                    talos_agent_api::AgentMessageStarted {
                        message: talos_agent_api::AgentMessage {
                            message_id: "agent-waiting".to_string(),
                            turn_id: identity.turn_id.clone(),
                            role: talos_agent_api::AgentMessageRole::Agent,
                            status: talos_agent_api::AgentMessageStatus::InProgress,
                            blocks: Vec::new(),
                            created_at: 2,
                            completed_at: None,
                        },
                    },
                ),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    admission = store
        .commit_progress_event(
            talos_agent_api::AgentEvent {
                identity: identity.clone(),
                sequence: admission.events.last().unwrap().sequence.saturating_add(1),
                occurred_at: 3,
                payload: talos_agent_api::AgentEventPayload::MessageBlockStarted(
                    talos_agent_api::AgentMessageBlockStarted {
                        message_id: "agent-waiting".to_string(),
                        block: talos_agent_api::AgentMessageBlock {
                            block_id: "tool-1".to_string(),
                            status: talos_agent_api::AgentMessageBlockStatus::Running,
                            content: talos_agent_api::AgentMessageBlockContent::ToolCall(
                                talos_agent_api::AgentToolCall {
                                    call_id: "call-1".to_string(),
                                    name: "AskUser".to_string(),
                                    arguments: talos_agent_api::AgentToolCallArguments::Streaming {
                                        json_fragment: "{}".to_string(),
                                    },
                                    progress: Vec::new(),
                                },
                            ),
                            started_at: 3,
                            completed_at: None,
                        },
                    },
                ),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    store
        .commit_waiting_settlement(
            talos_agent_api::AgentEvent {
                identity: identity.clone(),
                sequence: admission.events.last().unwrap().sequence.saturating_add(1),
                occurred_at: 2,
                payload: talos_agent_api::AgentEventPayload::WaitingForInput(
                    talos_agent_api::AgentPendingInput::UserAction {
                        action: PendingUserAction {
                            action_id: "action-1".to_string(),
                            call_id: "call-1".to_string(),
                            item_id: Some("tool-1".to_string()),
                            state: UserActionState::Pending,
                            params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
                                tool_name: "AskUser".to_string(),
                            }),
                        },
                    },
                ),
            },
            talos_agent_api::AgentContinuationRef {
                opaque_id: "continuation-waiting".to_string(),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new("session-waiting-cancel", "turn-waiting"),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        *cancelled_turn_ids.lock().unwrap(),
        vec!["turn-waiting".to_string()]
    );
    assert!(store
        .get_agent_active_turn("session-waiting-cancel")
        .await
        .unwrap()
        .is_none());
    let events = store.list_agent_turn_events(&identity, 32).await.unwrap();
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(talos_agent_api::AgentEventPayload::TurnFailed(failed))
            if failed.error.code == talos_error::ErrorCode::Cancelled
    ));
}

async fn commit_running_turn(
    store: &DatabaseSessionStore,
    session_id: &str,
    turn_id: &str,
    message_id: &str,
) -> talos_store::session::AgentEventCommit {
    let identity = talos_agent_api::AgentTurnRef::new(session_id, turn_id);
    let message = crate::turn::completed_agent_user_input(
        turn_id,
        message_id,
        crate::test_support::test_user_input("running"),
        1,
    )
    .unwrap();
    crate::test_support::commit_test_running_turn(store, identity, message).await
}
