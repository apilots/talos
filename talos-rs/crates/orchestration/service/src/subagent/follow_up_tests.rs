use super::*;

#[tokio::test]
async fn follow_up_reuses_the_child_agent_binding() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let child = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent.clone(),
            task_name: "research".to_string(),
            prompt: "Review the patch".to_string(),
        })
        .await
        .unwrap()
        .subagent;

    services
        .subagents
        .wait(wait_request(parent.clone(), &child, 1_000))
        .await
        .unwrap();

    let result = services
        .subagents
        .follow_up(FollowUpSubagentRequest {
            caller: parent,
            target: target(&child),
            prompt: "Check the tests too".to_string(),
        })
        .await
        .unwrap();

    let requests = services.executor.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].context.session.agent_id, "default");
    assert_eq!(
        message_text(&requests[1].input),
        Some("Check the tests too")
    );
    let expected_turn_id = &requests[1].context.turn_id;
    assert!(matches!(
        result.outcome,
        SubagentOutcome::Running { ref turn_id }
            | SubagentOutcome::Completed { ref turn_id, .. }
            if turn_id == expected_turn_id
    ));
}

#[tokio::test]
async fn follow_up_rejects_a_running_child() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _) = create_running_child(&services, parent.clone()).await;

    let error = services
        .subagents
        .follow_up(FollowUpSubagentRequest {
            caller: parent,
            target: target(&child),
            prompt: "Continue".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
    assert!(services.executor.requests.lock().unwrap().is_empty());
}
