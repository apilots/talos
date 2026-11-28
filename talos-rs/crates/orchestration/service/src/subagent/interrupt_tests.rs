use super::*;

#[tokio::test]
async fn interrupt_finalizes_the_canonical_child_turn() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent,
            target: target(&child),
        })
        .await
        .unwrap();

    assert_eq!(
        result.outcome,
        SubagentOutcome::Interrupted {
            turn_id: turn_id.clone(),
        }
    );
    assert_eq!(
        services
            .coordinator
            .cancellations
            .lock()
            .unwrap()
            .as_slice(),
        &[(child.session_id, turn_id, Some("reviewer".to_string()))]
    );
}

#[tokio::test]
async fn interrupt_waiting_child_clears_its_durable_continuation() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    let identity = AgentTurnRef::new(&child.session_id, &turn_id);
    let checkpoint = services
        .journal
        .checkpoint(&identity)
        .await
        .unwrap()
        .unwrap();
    crate::test_worker::seed_worker_events(
        services.sessions.as_ref(),
        services.store.as_ref(),
        services.journal.as_ref(),
        vec![CanonicalAgentEvent {
            identity: identity.clone(),
            sequence: checkpoint.last_sequence + 1,
            occurred_at: 2,
            payload: CanonicalAgentEventPayload::WaitingForInput(
                talos_agent_api::AgentPendingInput::Message {
                    action_id: "action-child".into(),
                    prompt: "Continue?".into(),
                },
            ),
        }],
        Some(talos_agent_api::AgentContinuationRef {
            opaque_id: "child-continuation".into(),
        }),
    )
    .await;
    let result = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent,
            target: target(&child),
        })
        .await
        .unwrap();
    assert_eq!(result.outcome, SubagentOutcome::Interrupted { turn_id });
    assert!(services
        .store
        .get_agent_active_turn(&identity.session_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn interrupt_retries_until_child_cleanup_is_confirmed() {
    let mut config = test_config();
    config.agent.subagent_admission.max_active_turns = std::num::NonZeroU32::MIN;
    let services = test_services_with_config(config).await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    services
        .coordinator
        .interrupt_outcome
        .lock()
        .unwrap()
        .replace(AgentInterruptOutcome::CleanupUnconfirmed { task_count: 1 });
    let error = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent.clone(),
            target: target(&child),
        })
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::RunCleanupUnconfirmed));
    assert!(
        services
            .store
            .get_agent_active_turn(&child.session_id)
            .await
            .unwrap()
            .unwrap()
            .cancellation_requested
    );
    let spawn = || {
        services.subagents.spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "next".to_string(),
            prompt: "Continue".to_string(),
        })
    };
    assert!(spawn()
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::SessionBudgetExceeded));
    services
        .coordinator
        .interrupt_outcome
        .lock()
        .unwrap()
        .replace(AgentInterruptOutcome::Cancelled);
    let result = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent.clone(),
            target: target(&child),
        })
        .await
        .unwrap();
    assert_eq!(result.outcome, SubagentOutcome::Interrupted { turn_id });
    assert_eq!(services.coordinator.cancellations.lock().unwrap().len(), 2);
    spawn().await.unwrap();
}

#[tokio::test]
async fn interrupt_is_idempotent_for_a_terminal_child() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let spawned = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap();
    let child = spawned.subagent;

    services
        .subagents
        .wait(wait_request(parent.clone(), &child, 1_000))
        .await
        .unwrap();

    let result = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent,
            target: target(&child),
        })
        .await
        .unwrap();

    assert!(matches!(result.outcome, SubagentOutcome::Completed { .. }));
    assert!(services
        .coordinator
        .cancellations
        .lock()
        .unwrap()
        .is_empty());
}
