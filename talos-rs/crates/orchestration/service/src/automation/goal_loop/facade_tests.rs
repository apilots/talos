use super::*;
use crate::automation::goal_loop::AutomationSkillResolver;
use talos_types::*;
struct TestSkills;
impl AutomationSkillResolver for TestSkills {
    fn resolve<'a>(
        &'a self,
        _session: &'a talos_store::session::SessionRecord,
        mode: AutomationMode,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<SkillSelector>> {
        Box::pin(async move {
            Ok(SkillSelector::selection(
                mode.skill_name(),
                format!("/verified-system/{}/SKILL.md", mode.skill_name()),
            ))
        })
    }
}
async fn fixture() -> (TalosService, Arc<DatabaseSessionStore>) {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    service
        .install_session_automation(store.automation_store(), Arc::new(TestSkills))
        .unwrap();
    for (id, user) in [("alice", "alice"), ("bob", "bob")] {
        service
            .create_session(
                &Actor::regular(user),
                create_session_params(id, request_source(), None),
            )
            .await
            .unwrap();
    }
    (service, store)
}
fn goal(id: &str, session: &str, status: GoalStatus) -> SessionGoal {
    let now = if status == GoalStatus::Complete {
        1
    } else {
        chrono::Utc::now().timestamp() - AUTOMATION_INITIAL_DELAY_SECONDS
    };
    SessionGoal {
        id: id.into(),
        session_id: session.into(),
        objective: "Finish migration".into(),
        objective_revision: 1,
        status,
        pause_reason: Some(GoalPauseReason::UserPaused),
        reason: None,
        token_budget: Some(150),
        tokens_used: 12,
        latest_turn_id: None,
        completion_evidence: None,
        created_at: now,
        updated_at: now,
    }
}
#[tokio::test]
async fn paginated_list_and_direct_snapshot_are_actor_scoped() {
    let (service, store) = fixture().await;
    let automation = store.automation_store();
    for (id, session) in [("g01", "alice"), ("g02", "alice"), ("g03", "bob")] {
        automation
            .create_goal(&goal(id, session, GoalStatus::Complete), None)
            .await
            .unwrap();
    }
    let request = AutomationListRequest {
        session_id: None,
        scope: AutomationListScope::Ended,
        cursor: None,
        limit: Some(1),
    };
    let first = service
        .list_automation(&Actor::regular("alice"), request.clone())
        .await
        .unwrap();
    assert_eq!(
        first.data,
        vec![AutomationListEntry::Goal {
            goal: goal("g01", "alice", GoalStatus::Complete)
        }]
    );
    assert_eq!(first.next_cursor, Some("g01".into()));
    let second = service
        .list_automation(
            &Actor::regular("alice"),
            AutomationListRequest {
                cursor: first.next_cursor,
                ..request
            },
        )
        .await
        .unwrap();
    assert_eq!(
        second.data,
        vec![AutomationListEntry::Goal {
            goal: goal("g02", "alice", GoalStatus::Complete)
        }]
    );
    assert_eq!(second.next_cursor, None);
    assert!(service
        .session_automation(&Actor::regular("alice"), "bob")
        .await
        .is_err());
    assert!(service
        .list_automation(
            &Actor::regular("alice"),
            AutomationListRequest {
                session_id: Some("bob".into()),
                scope: AutomationListScope::Ended,
                cursor: None,
                limit: None
            }
        )
        .await
        .is_err());
}
#[tokio::test]
async fn historical_goal_clear_does_not_mutate_current_goal() {
    let (service, store) = fixture().await;
    let automation = store.automation_store();
    let old = goal("old", "alice", GoalStatus::Complete);
    let current = goal("current", "alice", GoalStatus::Paused);
    automation.create_goal(&old, None).await.unwrap();
    automation.create_goal(&current, None).await.unwrap();
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Clear {
                        goal_id: "old".into(),
                    },
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(automation.goal("current").await.unwrap(), current);
    assert_eq!(
        automation.goal("old").await.unwrap().status,
        GoalStatus::Cleared
    );
}
#[tokio::test]
async fn editing_goal_preserves_budget_and_does_not_start_a_turn() {
    let (service, store) = fixture().await;
    let automation = store.automation_store();
    let before = goal("g", "alice", GoalStatus::Paused);
    automation.create_goal(&before, None).await.unwrap();
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Edit {
                        goal_id: "g".into(),
                        expected_revision: 1,
                        objective: "New objective".into(),
                    },
                },
            },
        )
        .await
        .unwrap();
    let after = automation.goal("g").await.unwrap();
    assert_eq!(after.token_budget, before.token_budget);
    assert_eq!(after.tokens_used, 12);
    assert_eq!(after.status, GoalStatus::Paused);
    assert_eq!(after.objective_revision, 2);
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
}

fn task(id: &str, mode: LoopSchedule) -> SessionLoop {
    // Existing behavior fixtures represent an already eligible definition.
    let now = chrono::Utc::now().timestamp() - AUTOMATION_INITIAL_DELAY_SECONDS;
    SessionLoop {
        id: id.into(),
        session_id: "alice".into(),
        prompt: "Check deployment".into(),
        schedule: mode,
        config_revision: 1,
        status: LoopStatus::Active,
        stop_reason: None,
        reason: None,
        next_due_at: None,
        latest_turn_id: None,
        created_at: now,
        updated_at: now,
    }
}
async fn admitted(
    service: &TalosService,
    store: &DatabaseSessionStore,
    origin: Option<AutomationTurnOrigin>,
    input: AgentUserInput,
) -> (AgentTurnRef, talos_store::session::AgentEventCommit) {
    let identity = AgentTurnRef::new("alice", format!("test-turn-{}", uuid::Uuid::new_v4()));
    let message = crate::turn::completed_agent_user_input(
        &identity.turn_id,
        "input-1",
        input,
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    let commit = store
        .commit_initial_admission(talos_store::session::CommitInitialAdmissionParams {
            automation_origin: origin,
            events: crate::test_support::canonical_user_admission(
                identity.clone(),
                message,
                1,
                true,
            ),
            command: None,
        })
        .await
        .unwrap();
    let _ = service;
    (identity, commit)
}
async fn normal_end(
    store: &DatabaseSessionStore,
    identity: AgentTurnRef,
    commit: talos_store::session::AgentEventCommit,
) -> CanonicalAgentEvent {
    let event = CanonicalAgentEvent {
        identity,
        sequence: commit.events.last().unwrap().sequence + 1,
        occurred_at: chrono::Utc::now().timestamp(),
        payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: talos_agent_api::AgentStopReason::EndTurn,
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
    event
}
#[tokio::test]
async fn dynamic_decision_schedules_next_run_after_normal_terminal() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    db.create_loop(&task("loop", LoopSchedule::ModelDecided {}))
        .await
        .unwrap();
    let (identity, commit) = admitted(
        &service,
        &store,
        Some(AutomationTurnOrigin::Loop {
            loop_id: "loop".into(),
            occurrence_id: "o".into(),
            config_revision: 1,
        }),
        crate::test_support::test_user_input("Check deployment"),
    )
    .await;
    let caller = AgentCaller {
        session_id: "alice".into(),
        turn_id: identity.turn_id.clone(),
        agent_id: store
            .get_session_record("alice")
            .await
            .unwrap()
            .unwrap()
            .agent_id,
    };
    service
        .execute_automation_model(AutomationModelRequest {
            caller,
            call_id: uuid::Uuid::new_v4().to_string(),
            action: AutomationAction::Loop {
                action: LoopAction::Decide {
                    loop_id: "loop".into(),
                    config_revision: 1,
                    decision: LoopWakeDecision::Continue {
                        delay_minutes: 2,
                        reason: "Waiting".into(),
                    },
                },
            },
        })
        .await
        .unwrap();
    assert_eq!(db.loop_task("loop").await.unwrap().next_due_at, None);
    let event = normal_end(&store, identity, commit).await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let automation = service.turns.automation_service().unwrap();
    automation
        .settle(&session, &event, Some(&AgentRunMetrics::default()))
        .await
        .unwrap();
    assert!(db.loop_task("loop").await.unwrap().next_due_at.unwrap() >= event.occurred_at + 120);
}
#[tokio::test]
async fn goal_budget_is_charged_from_bound_metrics_and_pauses_at_boundary() {
    let (service, store) = fixture().await;
    let input = AgentUserInput {
        content: vec![
            AgentUserContent::Text {
                text: "Finish tests".into(),
            },
            AgentUserContent::SkillSelection {
                name: "goal".into(),
                path: "/verified-system/goal/SKILL.md".into(),
            },
        ],
    };
    let (identity, commit) = admitted(&service, &store, None, input).await;
    let caller = AgentCaller {
        session_id: "alice".into(),
        turn_id: identity.turn_id.clone(),
        agent_id: store
            .get_session_record("alice")
            .await
            .unwrap()
            .unwrap()
            .agent_id,
    };
    let result = service
        .execute_automation_model(AutomationModelRequest {
            caller,
            call_id: "goal-create".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Create {
                    objective: "Finish tests".into(),
                    token_budget: Some(10),
                },
            },
        })
        .await
        .unwrap();
    let goal = result.snapshot.goal.unwrap();
    let binding = store
        .automation_store()
        .input_origin(&identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        binding.origin,
        AutomationTurnOrigin::Goal {
            goal_id: goal.id.clone(),
            objective_revision: 1
        }
    );
    let event = normal_end(&store, identity, commit).await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let mut metrics = AgentRunMetrics {
        tool_calls: 1,
        ..Default::default()
    };
    metrics.usage.total_tokens = 11;
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(&session, &event, Some(&metrics))
        .await
        .unwrap();
    let after = store.automation_store().goal(&goal.id).await.unwrap();
    assert_eq!(after.tokens_used, 11);
    assert_eq!(after.pause_reason, Some(GoalPauseReason::BudgetExhausted));
}
#[tokio::test]
async fn completed_goal_is_readable_but_not_a_mode_and_clear_hides_the_definition() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let mut complete = goal("complete", "alice", GoalStatus::Complete);
    complete.completion_evidence = Some("Tests passed".into());
    db.create_goal(&complete, None).await.unwrap();
    assert_eq!(
        service
            .session_automation(&Actor::regular("alice"), "alice")
            .await
            .unwrap()
            .goal,
        Some(complete.clone())
    );
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let automation = service.turns.automation_service().unwrap();
    automation
        .execute(
            &session,
            AutomationAction::Loop {
                action: LoopAction::Create {
                    prompt: "Check CI".into(),
                    schedule: LoopSchedule::ModelDecided {},
                },
            },
            crate::automation::goal_loop::AutomationCaller::User,
        )
        .await
        .unwrap();
    let l = db.loops("alice").await.unwrap().pop().unwrap();
    automation
        .execute(
            &session,
            AutomationAction::Loop {
                action: LoopAction::Stop { loop_id: l.id },
            },
            crate::automation::goal_loop::AutomationCaller::User,
        )
        .await
        .unwrap();
    automation
        .execute(
            &session,
            AutomationAction::Goal {
                action: GoalAction::Create {
                    objective: "New goal".into(),
                    token_budget: None,
                },
            },
            crate::automation::goal_loop::AutomationCaller::User,
        )
        .await
        .unwrap();
    let current = service
        .session_automation(&Actor::regular("alice"), "alice")
        .await
        .unwrap()
        .goal
        .unwrap();
    assert_ne!(current.id, complete.id);
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Clear {
                        goal_id: complete.id.clone(),
                    },
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .session_automation(&Actor::regular("alice"), "alice")
            .await
            .unwrap()
            .goal
            .unwrap()
            .id,
        current.id
    );
    let ended = service
        .list_automation(
            &Actor::regular("alice"),
            AutomationListRequest {
                session_id: None,
                scope: AutomationListScope::Ended,
                cursor: None,
                limit: None,
            },
        )
        .await
        .unwrap();
    assert!(!ended
        .data
        .iter()
        .any(|entry| matches!(entry,AutomationListEntry::Goal{goal} if goal.id==complete.id)));
}
#[tokio::test]
async fn automation_changes_have_typed_notifications_independent_of_queue_revision() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    db.create_goal(&goal("g", "alice", GoalStatus::Paused), None)
        .await
        .unwrap();
    let mut changes = service.subscribe_automation_changed().unwrap();
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Edit {
                        goal_id: "g".into(),
                        expected_revision: 1,
                        objective: "Changed goal".into(),
                    },
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        changes.try_recv().unwrap(),
        AutomationChangedNotification {
            session_id: "alice".into()
        }
    );
    assert_eq!(service.message_queue.list("alice").unwrap().revision, 0);
    let mut l = task("due", LoopSchedule::ModelDecided {});
    l.session_id = "bob".into();
    l.next_due_at = Some(1);
    db.create_loop(&l).await.unwrap();
    service.scan_session_loops(2).await.unwrap();
    assert_eq!(
        changes.try_recv().unwrap(),
        AutomationChangedNotification {
            session_id: "bob".into()
        }
    );
}
#[tokio::test]
async fn same_prompt_loops_read_the_exact_admitted_bound_origin() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    let first = task("first", LoopSchedule::ModelDecided {});
    let second = task("second", LoopSchedule::ModelDecided {});
    assert_eq!(first.prompt, second.prompt);
    db.create_loop(&first).await.unwrap();
    db.create_loop(&second).await.unwrap();
    let expected = AutomationTurnOrigin::Loop {
        loop_id: "second".into(),
        occurrence_id: "exact-occurrence".into(),
        config_revision: 1,
    };
    let (identity, _) = admitted(
        &service,
        &store,
        Some(expected.clone()),
        crate::test_support::test_user_input(second.prompt),
    )
    .await;
    let caller = AgentCaller {
        session_id: "alice".into(),
        turn_id: identity.turn_id,
        agent_id: store
            .get_session_record("alice")
            .await
            .unwrap()
            .unwrap()
            .agent_id,
    };
    for action in [
        AutomationAction::Goal {
            action: GoalAction::Read {},
        },
        AutomationAction::Loop {
            action: LoopAction::List {},
        },
    ] {
        let result = service
            .execute_automation_model(AutomationModelRequest {
                caller: caller.clone(),
                call_id: uuid::Uuid::new_v4().to_string(),
                action,
            })
            .await
            .unwrap();
        assert_eq!(result.bound_origin, Some(expected.clone()));
        assert_eq!(result.snapshot.loops.len(), 2);
    }
}
#[tokio::test]
async fn committed_user_steer_retains_bound_definition_management() {
    let (service, store) = fixture().await;
    let db = store.automation_store();
    db.create_goal(&goal("g", "alice", GoalStatus::Active), None)
        .await
        .unwrap();
    let (identity, commit) = admitted(
        &service,
        &store,
        Some(AutomationTurnOrigin::Goal {
            goal_id: "g".into(),
            objective_revision: 1,
        }),
        crate::test_support::test_user_input("Do goal work"),
    )
    .await;
    service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Pause {
                        goal_id: "g".into(),
                    },
                },
            },
        )
        .await
        .unwrap();
    assert!(!db.has_user_steer(&identity).await.unwrap());
    let message = crate::turn::completed_agent_user_input(
        &identity.turn_id,
        "steer-resume",
        crate::test_support::test_user_input("Continue this Goal"),
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    store
        .commit_running_input_admission(talos_store::session::CommitRunningInputAdmissionParams {
            events: crate::test_support::canonical_user_admission(
                identity.clone(),
                message,
                commit.events.last().unwrap().sequence + 1,
                false,
            ),
            expected_event_seq: commit.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap();
    assert!(db.has_user_steer(&identity).await.unwrap());
    let caller = AgentCaller {
        session_id: "alice".into(),
        turn_id: identity.turn_id,
        agent_id: store
            .get_session_record("alice")
            .await
            .unwrap()
            .unwrap()
            .agent_id,
    };
    service
        .execute_automation_model(AutomationModelRequest {
            caller: caller.clone(),
            call_id: "steer-management".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Resume {
                    goal_id: "g".into(),
                },
            },
        })
        .await
        .unwrap();
    assert_eq!(db.goal("g").await.unwrap().status, GoalStatus::Active);
    let rejected = service
        .execute_automation_model(AutomationModelRequest {
            caller,
            call_id: "forbidden-mode".into(),
            action: AutomationAction::Loop {
                action: LoopAction::Create {
                    prompt: "Other mode".into(),
                    schedule: LoopSchedule::ModelDecided {},
                },
            },
        })
        .await
        .unwrap_err();
    assert!(rejected.is_code(talos_error::ErrorCode::PermissionDenied));
}

#[path = "no_progress_tests.rs"]
mod no_progress_tests;

#[path = "reconciliation_tests.rs"]
mod reconciliation_tests;

#[path = "created_object_tests.rs"]
mod created_object_tests;

#[path = "lifetime_tests.rs"]
mod lifetime_tests;

#[path = "initial_delay_tests.rs"]
mod initial_delay_tests;

#[path = "cleared_binding_tests.rs"]
mod cleared_binding_tests;

#[path = "explicit_dispatch_tests.rs"]
mod explicit_dispatch_tests;

#[path = "preparation_tests.rs"]
mod preparation_tests;

#[path = "cross_runtime_tests.rs"]
mod cross_runtime_tests;
