use super::*;

#[tokio::test]
async fn cleared_goal_read_retains_exact_registration_or_work_binding() {
    for admitted_work in [false, true] {
        let (service, store) = fixture().await;
        let db = store.automation_store();
        let definition = goal("bound-goal", "alice", GoalStatus::Active);
        let origin = AutomationTurnOrigin::Goal {
            goal_id: definition.id.clone(),
            objective_revision: definition.objective_revision,
        };
        if admitted_work {
            db.create_goal(&definition, None).await.unwrap();
        }
        let input = AgentUserInput {
            content: vec![
                AgentUserContent::Text {
                    text: "Do Goal work".into(),
                },
                AgentUserContent::SkillSelection {
                    name: "goal".into(),
                    path: "/verified-system/goal/SKILL.md".into(),
                },
            ],
        };
        let (identity, commit) = admitted(
            &service,
            &store,
            admitted_work.then_some(origin.clone()),
            input,
        )
        .await;
        if !admitted_work {
            db.create_goal(&definition, Some(&identity)).await.unwrap();
        }
        service
            .execute_automation(
                &Actor::regular("alice"),
                AutomationRequest {
                    session_id: "alice".into(),
                    action: AutomationAction::Goal {
                        action: GoalAction::Clear {
                            goal_id: definition.id.clone(),
                        },
                    },
                },
            )
            .await
            .unwrap();
        assert!(db.snapshot("alice").await.unwrap().goal.is_none());
        let result = service
            .execute_automation_model(AutomationModelRequest {
                caller: AgentCaller {
                    session_id: "alice".into(),
                    turn_id: identity.turn_id.clone(),
                    agent_id: store
                        .get_session_record("alice")
                        .await
                        .unwrap()
                        .unwrap()
                        .agent_id,
                },
                call_id: "resumed-binding-read".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Read {},
                },
            })
            .await
            .unwrap();
        assert_eq!(result.bound_origin, Some(origin));
        assert_eq!(
            result.snapshot.goal,
            Some(db.goal(&definition.id).await.unwrap())
        );
        assert_eq!(
            result.snapshot.goal.unwrap().latest_turn_id,
            admitted_work.then_some(identity.turn_id.clone())
        );
        // A real registration remains a receipt after the user clears it.
        let event = normal_end(&store, identity, commit).await;
        service
            .turns
            .automation_service()
            .unwrap()
            .settle(
                &store.get_session_record("alice").await.unwrap().unwrap(),
                &event,
                None,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn explicit_goal_waiting_for_clarification_does_not_report_missing_registration() {
    let (service, store) = fixture().await;
    let input = AgentUserInput {
        content: vec![
            AgentUserContent::Text {
                text: "Work on this goal".into(),
            },
            AgentUserContent::SkillSelection {
                name: "goal".into(),
                path: "/verified-system/goal/SKILL.md".into(),
            },
        ],
    };
    let (identity, commit) = admitted(&service, &store, None, input).await;
    let waiting = CanonicalAgentEvent {
        identity,
        sequence: commit.events.last().unwrap().sequence + 1,
        occurred_at: chrono::Utc::now().timestamp(),
        payload: CanonicalAgentEventPayload::WaitingForInput(
            talos_agent_api::AgentPendingInput::Message {
                action_id: "clarify-goal".into(),
                prompt: "Which repository should the Goal use?".into(),
            },
        ),
    };
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(
            &store.get_session_record("alice").await.unwrap().unwrap(),
            &waiting,
            None,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn successful_loop_registration_is_not_missing_after_completion_or_clear() {
    for clear_before_completion in [false, true] {
        let (service, store) = fixture().await;
        let session = store.get_session_record("alice").await.unwrap().unwrap();
        let automation = service.turns.automation_service().unwrap();
        let input = automation
            .skill_input(&session, AutomationMode::Loop, "Check CI".into())
            .await
            .unwrap();
        let (identity, commit) = admitted(&service, &store, None, input).await;
        let created = service
            .execute_automation_model(AutomationModelRequest {
                caller: AgentCaller {
                    session_id: session.id.clone(),
                    turn_id: identity.turn_id.clone(),
                    agent_id: session.agent_id.clone(),
                },
                call_id: "register-loop".into(),
                action: AutomationAction::Loop {
                    action: LoopAction::Create {
                        prompt: "Check CI".into(),
                        schedule: LoopSchedule::ModelDecided {},
                    },
                },
            })
            .await
            .unwrap();
        let Some(AutomationObjectRef::Loop { loop_id }) = created.created_object else {
            panic!("expected a successful Loop creation receipt")
        };
        assert!(store
            .automation_store()
            .origin(&identity)
            .await
            .unwrap()
            .is_none());
        if clear_before_completion {
            service
                .execute_automation(
                    &Actor::regular("alice"),
                    AutomationRequest {
                        session_id: session.id.clone(),
                        action: AutomationAction::Loop {
                            action: LoopAction::Clear { loop_id },
                        },
                    },
                )
                .await
                .unwrap();
            assert!(store
                .automation_store()
                .snapshot(&session.id)
                .await
                .unwrap()
                .loops
                .is_empty());
        }
        let event = normal_end(&store, identity, commit).await;
        automation.settle(&session, &event, None).await.unwrap();
    }
}
