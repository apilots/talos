use super::*;

#[tokio::test]
async fn concurrent_same_prompt_model_creations_return_exact_loop_references() {
    let (service, store) = fixture().await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let input =
        crate::test_support::test_user_input("Set up repeated follow-ups for both deployments");
    let (identity, _) = admitted(&service, &store, None, input).await;
    let request = AutomationModelRequest {
        caller: AgentCaller {
            session_id: session.id,
            agent_id: session.agent_id,
            turn_id: identity.turn_id,
        },
        call_id: "first-creation".into(),
        action: AutomationAction::Loop {
            action: LoopAction::Create {
                prompt: "The same prompt cannot identify a Loop".into(),
                schedule: LoopSchedule::FixedInterval { minutes: 5 },
            },
        },
    };
    let (first, second) = tokio::join!(
        service.execute_automation_model(request.clone()),
        service.execute_automation_model(AutomationModelRequest {
            call_id: "second-creation".into(),
            ..request
        }),
    );
    let first = first.unwrap();
    let second = second.unwrap();
    let Some(AutomationObjectRef::Loop { loop_id: first_id }) = first.created_object else {
        panic!("missing exact first Loop reference")
    };
    let Some(AutomationObjectRef::Loop { loop_id: second_id }) = second.created_object else {
        panic!("missing exact second Loop reference")
    };
    assert_ne!(first_id, second_id);
    assert!(first.snapshot.loops.iter().any(|task| task.id == first_id));
    assert!(second
        .snapshot
        .loops
        .iter()
        .any(|task| task.id == second_id));
    let listed = service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Loop {
                    action: LoopAction::List {},
                },
            },
        )
        .await
        .unwrap();
    assert!(listed.created_object.is_none());
    assert_eq!(listed.snapshot.loops.len(), 2);
}

#[tokio::test]
async fn user_goal_creation_returns_exact_reference_even_when_progress_refreshes_snapshot() {
    let (service, _) = fixture().await;
    let result = service
        .execute_automation(
            &Actor::regular("alice"),
            AutomationRequest {
                session_id: "alice".into(),
                action: AutomationAction::Goal {
                    action: GoalAction::Create {
                        objective: "Complete the requested work".into(),
                        token_budget: None,
                    },
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        result.created_object,
        Some(AutomationObjectRef::Goal {
            goal_id: result.snapshot.goal.unwrap().id
        })
    );
}

#[tokio::test]
async fn retried_model_creation_returns_original_object_not_a_new_same_prompt_loop() {
    let (service, store) = fixture().await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    let input = service
        .turns
        .automation_service()
        .unwrap()
        .skill_input(
            &session,
            AutomationMode::Loop,
            "Explicitly create a Loop".into(),
        )
        .await
        .unwrap();
    let (identity, _) = admitted(&service, &store, None, input).await;
    let request = AutomationModelRequest {
        caller: AgentCaller {
            session_id: session.id,
            agent_id: session.agent_id,
            turn_id: identity.turn_id,
        },
        call_id: "one-creation-call".into(),
        action: AutomationAction::Loop {
            action: LoopAction::Create {
                prompt: "same prompt".into(),
                schedule: LoopSchedule::FixedInterval { minutes: 5 },
            },
        },
    };
    let first = service
        .execute_automation_model(request.clone())
        .await
        .unwrap();
    let repeated = service
        .execute_automation_model(request.clone())
        .await
        .unwrap();
    assert!(matches!(
        first.created_object,
        Some(AutomationObjectRef::Loop { .. })
    ));
    assert_eq!(repeated.created_object, first.created_object);
    assert_eq!(repeated.snapshot.loops.len(), 1);
    let second = service
        .execute_automation_model(AutomationModelRequest {
            call_id: "another-creation-call".into(),
            ..request
        })
        .await
        .unwrap();
    assert_ne!(second.created_object, first.created_object);
    assert_eq!(second.snapshot.loops.len(), 2);
}
