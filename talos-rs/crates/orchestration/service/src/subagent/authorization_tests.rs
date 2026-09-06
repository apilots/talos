use super::*;

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
        services.store.as_ref(),
        services.timeline.as_ref(),
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
