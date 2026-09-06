use super::*;

#[tokio::test]
async fn cancellation_admission_moves_the_exact_running_turn_to_cancelling() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();

    let cancellation = store
        .begin_cancellation(&admission.active_turn.identity)
        .await
        .unwrap();

    assert_eq!(
        (
            cancellation.previous_state,
            cancellation.active_turn.state,
            store
                .get_agent_active_turn("session-1")
                .await
                .unwrap()
                .unwrap()
                .state,
        ),
        (
            ActiveTurnState::Running,
            ActiveTurnState::Cancelling,
            ActiveTurnState::Cancelling,
        )
    );
}

#[tokio::test]
async fn cancellation_admission_preserves_the_latest_durable_tail() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let progress = store
        .commit_progress_event(
            event(
                3,
                AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
            ),
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    let cancellation = store
        .begin_cancellation(&progress.active_turn.identity)
        .await
        .unwrap();

    assert_eq!(
        (
            cancellation.active_turn.last_event_seq,
            cancellation.active_turn.state,
        ),
        (
            progress.active_turn.last_event_seq,
            ActiveTurnState::Cancelling,
        )
    );
}

#[tokio::test]
async fn runtime_rejection_restores_only_the_admitted_cancellation() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let cancellation = store
        .begin_cancellation(&admission.active_turn.identity)
        .await
        .unwrap();

    let restored = store.restore_cancellation(cancellation).await.unwrap();

    assert_eq!(restored.state, ActiveTurnState::Running);
    assert_eq!(
        store
            .get_agent_active_turn("session-1")
            .await
            .unwrap()
            .unwrap(),
        restored
    );
}

#[tokio::test]
async fn runtime_rejection_restores_state_without_discarding_late_progress() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let cancellation = store
        .begin_cancellation(&admission.active_turn.identity)
        .await
        .unwrap();
    let progress = store
        .commit_progress_event(
            event(
                3,
                AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
            ),
            cancellation.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    let restored = store.restore_cancellation(cancellation).await.unwrap();

    assert_eq!(
        (restored.state, restored.last_event_seq),
        (
            ActiveTurnState::Running,
            progress.active_turn.last_event_seq
        )
    );
}

#[tokio::test]
async fn cancelling_turn_accepts_provider_progress_but_rejects_new_input() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let cancellation = store
        .begin_cancellation(&admission.active_turn.identity)
        .await
        .unwrap();
    let progress = store
        .commit_progress_event(
            event(
                3,
                AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
            ),
            cancellation.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    let input_error = store
        .commit_running_input_admission(CommitRunningInputAdmissionParams {
            events: user_admission(4, "message-2"),
            expected_event_seq: progress.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap_err();

    assert_eq!(progress.active_turn.state, ActiveTurnState::Cancelling);
    assert!(input_error.is_code(talos_error::ErrorCode::Conflict));
}

#[tokio::test]
async fn natural_terminal_wins_over_a_stale_cancellation_admission() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                3,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            ),
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();

    let error = store
        .begin_cancellation(&admission.active_turn.identity)
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        None
    );
}
