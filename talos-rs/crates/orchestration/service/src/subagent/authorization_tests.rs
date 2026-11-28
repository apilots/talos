use super::*;

#[tokio::test]
async fn caller_authorization_observes_worker_admission_before_live_projection() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let child = create_child_session(&services, &parent, "unprojected-child").await;
    let caller = AgentCaller {
        session_id: child.id.clone(),
        turn_id: "unprojected-turn".to_string(),
        agent_id: child.agent_id.clone(),
    };
    let context = services
        .sessions
        .build_agent_context(&child, caller.turn_ref())
        .await
        .unwrap();
    let input = crate::turn::completed_agent_user_input(
        &caller.turn_id,
        "unprojected-input",
        crate::test_support::test_user_input("spawn before product delivery"),
        1,
    )
    .unwrap();
    let events = canonical_user_admission(caller.turn_ref(), input.clone(), 1, true);
    let mut reducer = talos_agent::AgentEventReducer::new(caller.turn_ref());
    for event in &events {
        reducer.apply(event.clone()).unwrap();
    }
    services
        .journal
        .admit(
            &context.session,
            &agent_protocol::AgentExecutionCommand::Run {
                params: Box::new(AgentRunParams {
                    context: context.clone(),
                    input,
                    subagent_limit: None,
                    command: None,
                    automation_origin: None,
                    intent: agent_protocol::AgentRunIntent::Start,
                }),
            },
            events,
            &reducer.checkpoint(),
        )
        .await
        .unwrap();
    assert!(services
        .store
        .get_agent_active_turn(&child.id)
        .await
        .unwrap()
        .is_none());

    let mut another_agent = caller.clone();
    another_agent.agent_id = "another-agent".to_string();
    let rejected = services
        .subagents
        .list(ListSubagentsRequest {
            caller: another_agent,
        })
        .await;
    assert!(rejected.is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(services
        .store
        .worker_event_cursor(&child.id)
        .await
        .unwrap()
        .is_none());

    let tree = services
        .subagents
        .list(ListSubagentsRequest { caller })
        .await
        .unwrap();

    assert!(tree.children.is_empty());
}

#[tokio::test]
async fn caller_authorization_rejects_worker_terminal_before_live_projection() {
    let services = test_services().await;
    let caller = create_parent(&services).await;
    let record = services
        .sessions
        .load_record(&caller.session_id)
        .await
        .unwrap()
        .unwrap();
    let context = services
        .sessions
        .build_agent_session_context(&record)
        .await
        .unwrap();
    let checkpoint = services
        .journal
        .checkpoint(&caller.turn_ref())
        .await
        .unwrap()
        .unwrap();
    let event = CanonicalAgentEvent {
        identity: caller.turn_ref(),
        sequence: checkpoint.last_sequence + 1,
        occurred_at: 2,
        payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    };
    let mut reducer =
        talos_agent::AgentEventReducer::from_checkpoint(caller.turn_ref(), checkpoint).unwrap();
    reducer.apply(event.clone()).unwrap();
    services
        .journal
        .append(
            &context,
            vec![event],
            &reducer.checkpoint(),
            Some(agent_protocol::AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: AgentRunMetrics::default(),
            }),
        )
        .await
        .unwrap();
    assert!(services
        .store
        .get_agent_active_turn(&caller.session_id)
        .await
        .unwrap()
        .is_some());

    let result = services
        .subagents
        .list(ListSubagentsRequest { caller })
        .await;

    assert!(result.is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn caller_authorization_rejects_a_different_turn_after_refresh() {
    let services = test_services().await;
    let mut caller = create_parent(&services).await;
    caller.turn_id = "different-turn".to_string();

    let result = services
        .subagents
        .list(ListSubagentsRequest { caller })
        .await;

    assert!(result.is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn caller_authorization_preserves_product_cancellation_after_refresh() {
    let services = test_services().await;
    let caller = create_parent(&services).await;
    services
        .store
        .begin_cancellation(&caller.turn_ref())
        .await
        .unwrap();

    let result = services
        .subagents
        .list(ListSubagentsRequest { caller })
        .await;

    assert!(result.is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn every_existing_child_operation_rejects_a_stale_caller_turn() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let spawned = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review".to_string(),
        })
        .await
        .unwrap();
    let child = spawned.subagent;
    settle_canonical_turn(
        services.sessions.as_ref(),
        services.store.as_ref(),
        services.timeline.as_ref(),
        services.journal.as_ref(),
        &parent.session_id,
        &parent.turn_id,
        CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    )
    .await;

    let wait = services
        .subagents
        .wait(wait_request(parent.clone(), &child, 0))
        .await;
    let interrupt = services
        .subagents
        .interrupt(InterruptSubagentRequest {
            caller: parent.clone(),
            target: target(&child),
        })
        .await;
    let follow_up = services
        .subagents
        .follow_up(FollowUpSubagentRequest {
            caller: parent.clone(),
            target: target(&child),
            prompt: "continue".to_string(),
        })
        .await;
    let list = services
        .subagents
        .list(ListSubagentsRequest { caller: parent })
        .await;

    assert!(wait
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(interrupt
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(follow_up
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(list
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}
