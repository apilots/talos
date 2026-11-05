//! First automatic admission uses existing definition timestamps and cancellation.
use super::*;

pub(super) async fn create(
    service: &TalosService,
    action: AutomationAction,
) -> AutomationControlResult {
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action,
            },
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn original_scan_admits_only_due_initial_goal_once() {
    let (service, store) = fixture().await;
    store
        .automation_store()
        .create_goal(&goal("g", "alice", GoalStatus::Active), None)
        .await
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    service.scan_session_loops(now).await.unwrap();
    let active = store.get_agent_active_turn("alice").await.unwrap().unwrap();
    assert_eq!(
        store
            .automation_store()
            .goal("g")
            .await
            .unwrap()
            .latest_turn_id,
        Some(active.identity.turn_id.clone())
    );
    service.scan_session_loops(now + 1).await.unwrap();
    assert_eq!(
        store
            .get_agent_active_turn("alice")
            .await
            .unwrap()
            .unwrap()
            .identity,
        active.identity
    );
    assert!(store
        .automation_store()
        .initial_goal_sessions(now + 1)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn initial_loop_due_scan_admits_once_then_uses_original_minute_cadence() {
    let (service, store) = fixture().await;
    let deadline = chrono::Utc::now().timestamp();
    let mut task = task("due-loop", LoopSchedule::FixedInterval { minutes: 1 });
    task.created_at = deadline - AUTOMATION_INITIAL_DELAY_SECONDS;
    task.next_due_at = Some(deadline);
    store.automation_store().create_loop(&task).await.unwrap();
    service.scan_session_loops(deadline - 1).await.unwrap();
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert!(service
        .message_queue
        .list("alice")
        .unwrap()
        .messages
        .is_empty());
    service.scan_session_loops(deadline).await.unwrap();
    let active = store.get_agent_active_turn("alice").await.unwrap().unwrap();
    assert_eq!(
        store
            .automation_store()
            .loop_task(&task.id)
            .await
            .unwrap()
            .latest_turn_id,
        Some(active.identity.turn_id.clone())
    );
    assert!(service
        .message_queue
        .list("alice")
        .unwrap()
        .messages
        .is_empty());
    let terminal = CanonicalAgentEvent {
        identity: active.identity.clone(),
        sequence: u64::try_from(active.last_event_seq).unwrap() + 1,
        occurred_at: chrono::Utc::now().timestamp(),
        payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: talos_agent_api::AgentStopReason::EndTurn,
        }),
    };
    store
        .commit_terminal_settlement(talos_store::session::CommitTerminalSettlementParams {
            event: terminal.clone(),
            expected_event_seq: active.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(&session, &terminal, Some(&Default::default()))
        .await
        .unwrap();
    let after = store.automation_store().loop_task(&task.id).await.unwrap();
    assert!(after.next_due_at.unwrap() >= terminal.occurred_at + 60);
}
