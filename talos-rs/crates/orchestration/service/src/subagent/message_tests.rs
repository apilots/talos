use super::*;

#[tokio::test]
async fn send_message_dispatches_to_the_exact_running_child_turn() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_live_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .send_message(SendSubagentMessageRequest {
            caller: parent,
            target: target(&child),
            message: "Focus on tests".to_string(),
        })
        .await
        .unwrap();
    let inputs = services.executor.inputs.lock().unwrap();

    assert!(result.accepted);
    assert_eq!(result.subagent, child);
    assert_eq!(result.agent_id, "reviewer");
    assert_eq!(result.turn.turn_id, turn_id);
    assert_eq!(inputs[0].agent_id, "reviewer");
    assert_eq!(inputs[0].identity.turn_id, turn_id);
    assert_eq!(message_text(&inputs[0].input), Some("Focus on tests"));
}

#[tokio::test]
async fn send_message_rejects_a_non_running_child() {
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

    services
        .subagents
        .wait(wait_request(parent.clone(), &spawned.subagent, 1_000))
        .await
        .unwrap();

    let error = services
        .subagents
        .send_message(SendSubagentMessageRequest {
            caller: parent,
            target: target(&spawned.subagent),
            message: "Too late".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(services.executor.inputs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn send_message_propagates_a_closed_harness_input() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _turn_id) = create_live_running_child(&services, parent.clone()).await;
    *services.executor.input_error.lock().unwrap() = Some(TalosError::new(
        talos_error::ErrorCode::ResourceNotFound,
        "message input closed".to_string(),
    ));

    let error = services
        .subagents
        .send_message(SendSubagentMessageRequest {
            caller: parent,
            target: target(&child),
            message: "Too late".to_string(),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert_eq!(error.diagnostic_message(), "message input closed");
}
