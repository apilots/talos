use super::*;

#[tokio::test]
async fn list_returns_the_complete_deterministic_agent_tree() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, child_turn_id) = create_running_child(&services, parent.clone()).await;
    let grandchild = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: AgentCaller {
                session_id: child.session_id.clone(),
                turn_id: child_turn_id,
                agent_id: "reviewer".to_string(),
            },
            task_name: "analysis".to_string(),
            prompt: "Inspect the API".to_string(),
        })
        .await
        .unwrap();
    let tree = services
        .subagents
        .list(ListSubagentsRequest { caller: parent })
        .await
        .unwrap();

    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].snapshot.subagent, child);
    assert_eq!(tree.children[0].children.len(), 1);
    assert_eq!(
        tree.children[0].children[0].snapshot.subagent,
        grandchild.subagent
    );
}

#[tokio::test]
async fn list_from_a_subagent_exposes_only_its_descendants() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, child_turn_id) = create_running_child(&services, parent.clone()).await;
    let grandchild = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: AgentCaller {
                session_id: child.session_id.clone(),
                turn_id: child_turn_id.clone(),
                agent_id: "reviewer".to_string(),
            },
            task_name: "analysis".to_string(),
            prompt: "Inspect the API".to_string(),
        })
        .await
        .unwrap();
    let sibling = services
        .subagents
        .spawn(SpawnSubagentRequest {
            caller: parent,
            task_name: "implementation".to_string(),
            prompt: "Implement the API".to_string(),
        })
        .await
        .unwrap();

    let tree = services
        .subagents
        .list(ListSubagentsRequest {
            caller: AgentCaller {
                session_id: child.session_id,
                turn_id: child_turn_id,
                agent_id: "reviewer".to_string(),
            },
        })
        .await
        .unwrap();

    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].snapshot.subagent, grandchild.subagent);
    assert_ne!(tree.children[0].snapshot.subagent, sibling.subagent);
}

#[tokio::test]
async fn list_rejects_an_agent_tree_beyond_the_response_limit() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    for index in 0..20 {
        let (child, turn_id) = create_running_child_named(
            &services,
            parent.clone(),
            &format!("response-child-{index}"),
        )
        .await;
        services
            .timeline
            .record_talos_event(AgentEvent {
                event_seq: None,
                event_id: None,
                created_at_ms: None,
                event_type: AgentEventType::ItemStarted,
                turn_id: turn_id.clone(),
                message_id: format!("message-child-{index}"),
                session_id: child.session_id,
                sequence: 1,
                payload: AgentEventPayload::ItemStarted(TalosItemStartedEvent {
                    request_id: format!("request-child-{index}"),
                    turn_id,
                    item: TalosTimelineItem {
                        item_id: format!("commentary-{index}"),
                        parent_item_id: None,
                        kind: TalosTimelineItemKind::AssistantText,
                        phase: TalosTimelinePhase::Commentary,
                        status: TalosTimelineItemStatus::Running,
                        title: None,
                        started_at_ms: 2,
                        completed_at_ms: None,
                        content: TalosTimelineContent::Text {
                            text: "x".repeat(2 * 1024),
                        },
                    },
                }),
            })
            .await
            .unwrap();
    }

    let error = services
        .subagents
        .list(ListSubagentsRequest { caller: parent })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}
