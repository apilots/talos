//! Goal natural duration and indefinite Loop lifecycle use the original state owner.
use super::*;
use crate::automation::goal_loop::AutomationCaller;

async fn model_request(
    service: &TalosService,
    store: &DatabaseSessionStore,
    turn: &AgentTurnRef,
    action: AutomationAction,
) -> TalosResult<AutomationControlResult> {
    service
        .execute_automation_model(AutomationModelRequest {
            caller: AgentCaller {
                session_id: turn.session_id.clone(),
                turn_id: turn.turn_id.clone(),
                agent_id: store
                    .get_session_record(&turn.session_id)
                    .await?
                    .unwrap()
                    .agent_id,
            },
            call_id: uuid::Uuid::new_v4().to_string(),
            action,
        })
        .await
}

#[tokio::test]
async fn goal_pause_resume_and_edit_keep_natural_start_without_completing() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let original = goal("g", "alice", GoalStatus::Active);
    db.create_goal(&original, None).await.unwrap();
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let automation = service.turns.automation_service().unwrap();
    for action in [
        GoalAction::Pause {
            goal_id: "g".into(),
        },
        GoalAction::Edit {
            goal_id: "g".into(),
            expected_revision: 1,
            objective: "Updated acceptance".into(),
        },
        GoalAction::Resume {
            goal_id: "g".into(),
        },
    ] {
        automation
            .execute(
                &session,
                AutomationAction::Goal { action },
                AutomationCaller::User,
            )
            .await
            .unwrap();
        let current = db.goal("g").await.unwrap();
        assert_eq!(current.created_at, original.created_at);
        assert_ne!(current.status, GoalStatus::Complete);
        assert_eq!(current.token_budget, original.token_budget);
    }
    // Clearing an unfinished definition hides it; it is not a completion event.
    automation
        .execute(
            &session,
            AutomationAction::Goal {
                action: GoalAction::Clear {
                    goal_id: "g".into(),
                },
            },
            AutomationCaller::User,
        )
        .await
        .unwrap();
    let cleared = db.goal("g").await.unwrap();
    assert_eq!(cleared.created_at, original.created_at);
    assert_eq!(cleared.status, GoalStatus::Cleared);
    assert!(db.snapshot("alice").await.unwrap().goal.is_none());
}

#[tokio::test]
async fn goal_completion_end_is_stable_across_turns_usage_duplicates_and_clear() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let original = goal("g", "alice", GoalStatus::Active);
    db.create_goal(&original, None).await.unwrap();
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let automation = service.turns.automation_service().unwrap();
    let origin = Some(AutomationTurnOrigin::Goal {
        goal_id: "g".into(),
        objective_revision: 1,
    });
    let (first, commit) = admitted(
        &service,
        &store,
        origin.clone(),
        crate::test_support::test_user_input("First work"),
    )
    .await;
    let first_end = normal_end(&store, first, commit).await;
    automation.settle(&session, &first_end, None).await.unwrap();
    assert_eq!(db.goal("g").await.unwrap().status, GoalStatus::Active);
    let (second, commit) = admitted(
        &service,
        &store,
        origin,
        crate::test_support::test_user_input("Finish work"),
    )
    .await;
    model_request(
        &service,
        &store,
        &second,
        AutomationAction::Goal {
            action: GoalAction::Complete {
                goal_id: "g".into(),
                objective_revision: 1,
                evidence: "Verified acceptance".into(),
            },
        },
    )
    .await
    .unwrap();
    // A declaration is not a completed Goal and must not freeze its duration.
    assert_eq!(db.goal("g").await.unwrap().status, GoalStatus::Active);
    let second_end = normal_end(&store, second.clone(), commit).await;
    let before = chrono::Utc::now().timestamp();
    automation
        .settle(&session, &second_end, None)
        .await
        .unwrap();
    let complete = db.goal("g").await.unwrap();
    let end = complete.updated_at;
    assert!(end >= before && end <= chrono::Utc::now().timestamp());
    assert_eq!(complete.created_at, original.created_at);
    assert_eq!(complete.status, GoalStatus::Complete);
    db.account_usage(&second, 20, end + 100).await.unwrap();
    db.account_usage(&second, 20, end + 200).await.unwrap();
    automation
        .settle(&session, &second_end, None)
        .await
        .unwrap();
    let visible = service
        .session_automation(&Actor::regular("alice"), "alice")
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_eq!(visible.updated_at, end);
    assert_eq!(visible.status, GoalStatus::Complete);
    automation
        .execute(
            &session,
            AutomationAction::Goal {
                action: GoalAction::Clear {
                    goal_id: "g".into(),
                },
            },
            AutomationCaller::User,
        )
        .await
        .unwrap();
    let cleared = db.goal("g").await.unwrap();
    assert!(db.snapshot("alice").await.unwrap().goal.is_none());
    assert_eq!(cleared.created_at, original.created_at);
    assert_eq!(cleared.tokens_used, original.tokens_used + 20);
}

#[tokio::test]
async fn fixed_loop_keeps_scheduling_after_old_age_and_rejects_automatic_stop() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let mut old = task("fixed", LoopSchedule::FixedInterval { minutes: 2 });
    old.created_at = 1;
    old.updated_at = 1;
    db.create_loop(&old).await.unwrap();
    let (turn, commit) = admitted(
        &service,
        &store,
        Some(AutomationTurnOrigin::Loop {
            loop_id: old.id.clone(),
            occurrence_id: "occ".into(),
            config_revision: 1,
        }),
        crate::test_support::test_user_input("Fixed work"),
    )
    .await;
    let error = model_request(
        &service,
        &store,
        &turn,
        AutomationAction::Loop {
            action: LoopAction::Stop {
                loop_id: old.id.clone(),
            },
        },
    )
    .await
    .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::PermissionDenied));
    let event = normal_end(&store, turn, commit).await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(&session, &event, Some(&AgentRunMetrics::default()))
        .await
        .unwrap();
    let scheduled = db.loop_task(&old.id).await.unwrap();
    assert_eq!(scheduled.status, LoopStatus::Active);
    assert_eq!(scheduled.next_due_at, Some(event.occurred_at + 120));
    assert_eq!(scheduled.stop_reason, None);
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Loop {
                    action: LoopAction::Stop {
                        loop_id: old.id.clone(),
                    },
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        db.loop_task(&old.id).await.unwrap().stop_reason,
        Some(LoopStopReason::UserStopped)
    );
}

#[tokio::test]
async fn model_decided_stop_is_applied_only_after_normal_completion() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    db.create_loop(&task("dynamic", LoopSchedule::ModelDecided {}))
        .await
        .unwrap();
    let (turn, commit) = admitted(
        &service,
        &store,
        Some(AutomationTurnOrigin::Loop {
            loop_id: "dynamic".into(),
            occurrence_id: "occ".into(),
            config_revision: 1,
        }),
        crate::test_support::test_user_input("Observe once"),
    )
    .await;
    model_request(
        &service,
        &store,
        &turn,
        AutomationAction::Loop {
            action: LoopAction::Decide {
                loop_id: "dynamic".into(),
                config_revision: 1,
                decision: LoopWakeDecision::Stop {
                    reason: "Condition resolved".into(),
                },
            },
        },
    )
    .await
    .unwrap();
    assert_eq!(
        db.loop_task("dynamic").await.unwrap().status,
        LoopStatus::Active
    );
    let event = normal_end(&store, turn, commit).await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(&session, &event, Some(&AgentRunMetrics::default()))
        .await
        .unwrap();
    let stopped = db.loop_task("dynamic").await.unwrap();
    assert_eq!(stopped.status, LoopStatus::Stopped);
    assert_eq!(stopped.stop_reason, Some(LoopStopReason::ModelStopped));
    assert_eq!(stopped.next_due_at, None);
}

#[tokio::test]
async fn goal_runtime_budget_pauses_continuation_without_resetting_its_start() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let mut original = goal("bounded", "alice", GoalStatus::Active);
    original.token_budget = None;
    original.created_at = chrono::Utc::now().timestamp() - 86_400;
    db.create_goal(&original, None).await.unwrap();
    let (turn, commit) = admitted(
        &service,
        &store,
        Some(AutomationTurnOrigin::Goal {
            goal_id: original.id.clone(),
            objective_revision: original.objective_revision,
        }),
        crate::test_support::test_user_input("Continue the bounded work"),
    )
    .await;
    let event = normal_end(&store, turn, commit).await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let automation = service.turns.automation_service().unwrap();
    automation.settle(&session, &event, None).await.unwrap();
    let paused = db.goal(&original.id).await.unwrap();
    assert_eq!(paused.status, GoalStatus::Paused);
    assert_eq!(paused.pause_reason, Some(GoalPauseReason::BudgetExhausted));
    automation
        .execute(
            &session,
            AutomationAction::Goal {
                action: GoalAction::Resume {
                    goal_id: original.id.clone(),
                },
            },
            AutomationCaller::User,
        )
        .await
        .unwrap();
    service.turns.continue_goal(session, true).await.unwrap();
    let resumed = db.goal(&original.id).await.unwrap();
    assert_eq!(resumed.status, GoalStatus::Paused);
    assert_eq!(resumed.pause_reason, Some(GoalPauseReason::BudgetExhausted));
    assert_eq!(resumed.created_at, original.created_at);
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn unrelated_user_failure_preserves_the_active_goal() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let mut original = goal("active", "alice", GoalStatus::Active);
    original.created_at = chrono::Utc::now().timestamp();
    db.create_goal(&original, None).await.unwrap();
    let (identity, commit) = admitted(
        &service,
        &store,
        None,
        crate::test_support::test_user_input("Answer an unrelated question"),
    )
    .await;
    let event = CanonicalAgentEvent {
        identity,
        sequence: commit.events.last().unwrap().sequence + 1,
        occurred_at: chrono::Utc::now().timestamp(),
        payload: CanonicalAgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
            error: talos_error::TalosError::new(
                talos_error::ErrorCode::InternalError,
                "The ordinary User task failed.",
            )
            .to_api_error(),
            retryable: false,
        }),
    };
    store
        .commit_terminal_settlement(talos_store::session::CommitTerminalSettlementParams {
            event: event.clone(),
            expected_event_seq: commit.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(&session, &event, None)
        .await
        .unwrap();
    assert_eq!(db.goal(&original.id).await.unwrap(), original);
}
