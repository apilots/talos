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
async fn initial_goal_waits_across_reads_and_scan_and_clear_prevents_admission() {
    let (service, store) = fixture().await;
    let result = create(
        &service,
        AutomationAction::Goal {
            action: GoalAction::Create {
                objective: "Finish tests".into(),
                token_budget: None,
            },
        },
    )
    .await;
    let registered = result.snapshot.goal.unwrap();
    let mut goal = registered.clone();
    // Pin the before-deadline fixture independently of test execution speed.
    goal.created_at = chrono::Utc::now().timestamp() + 86_400;
    store
        .automation_store()
        .replace_goal(&registered, &goal)
        .await
        .unwrap();
    assert_eq!(goal.latest_turn_id, None);
    assert_eq!(goal.status, GoalStatus::Active);
    service.reconcile_session_goal("alice").await.unwrap();
    service
        .scan_session_loops(goal.created_at + AUTOMATION_INITIAL_DELAY_SECONDS - 1)
        .await
        .unwrap();
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .automation_store()
        .initial_goal_sessions(goal.created_at + 9)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .automation_store()
            .initial_goal_sessions(goal.created_at + 10)
            .await
            .unwrap(),
        vec!["alice".to_owned()]
    );
    // Admission repeats the same gate inside its original SQLite transaction.
    let origin = AutomationTurnOrigin::Goal {
        goal_id: goal.id.clone(),
        objective_revision: 1,
    };
    let identity = AgentTurnRef::new("alice", "early-goal");
    let message = crate::turn::completed_agent_user_input(
        "early-goal",
        "input",
        crate::test_support::test_user_input("Work"),
        goal.created_at,
    )
    .unwrap();
    assert!(store
        .commit_initial_admission(talos_store::session::CommitInitialAdmissionParams {
            automation_origin: Some(origin),
            events: crate::test_support::canonical_user_admission(identity, message, 1, true),
            command: None,
        })
        .await
        .is_err());
    create(
        &service,
        AutomationAction::Goal {
            action: GoalAction::Clear { goal_id: goal.id },
        },
    )
    .await;
    service
        .scan_session_loops(goal.created_at + 100)
        .await
        .unwrap();
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn initial_loop_edit_keeps_deadline_and_stop_retracts_before_any_work() {
    let (service, store) = fixture().await;
    let result = create(
        &service,
        AutomationAction::Loop {
            action: LoopAction::Create {
                prompt: "Check CI".into(),
                schedule: LoopSchedule::FixedInterval { minutes: 1 },
            },
        },
    )
    .await;
    let task = result.snapshot.loops[0].clone();
    assert_eq!(task.latest_turn_id, None);
    assert_eq!(
        task.next_due_at,
        Some(task.created_at + AUTOMATION_INITIAL_DELAY_SECONDS)
    );
    let registered = task;
    let mut task = registered.clone();
    task.created_at = chrono::Utc::now().timestamp() + 86_400;
    task.next_due_at = Some(task.created_at + AUTOMATION_INITIAL_DELAY_SECONDS);
    store
        .automation_store()
        .replace_loop(&registered, &task)
        .await
        .unwrap();
    assert!(service
        .message_queue
        .list("alice")
        .unwrap()
        .messages
        .is_empty());
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    for (revision, schedule) in [
        (1, LoopSchedule::ModelDecided {}),
        (2, LoopSchedule::FixedInterval { minutes: 5 }),
    ] {
        create(
            &service,
            AutomationAction::Loop {
                action: LoopAction::Edit {
                    loop_id: task.id.clone(),
                    expected_revision: revision,
                    prompt: "New task".into(),
                    schedule,
                },
            },
        )
        .await;
        assert_eq!(
            store
                .automation_store()
                .loop_task(&task.id)
                .await
                .unwrap()
                .next_due_at,
            task.next_due_at
        );
        assert!(service
            .message_queue
            .list("alice")
            .unwrap()
            .messages
            .is_empty());
    }
    service
        .scan_session_loops(task.created_at + 9)
        .await
        .unwrap();
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    create(
        &service,
        AutomationAction::Loop {
            action: LoopAction::Stop {
                loop_id: task.id.clone(),
            },
        },
    )
    .await;
    service
        .scan_session_loops(task.created_at + 100)
        .await
        .unwrap();
    assert!(service
        .message_queue
        .list("alice")
        .unwrap()
        .messages
        .is_empty());
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .automation_store()
            .loop_task(&task.id)
            .await
            .unwrap()
            .status,
        LoopStatus::Stopped
    );
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
