use super::*;

#[tokio::test]
async fn wait_returns_an_already_completed_child() {
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

    let result = services
        .subagents
        .wait(wait_request(parent, &spawned.subagent, 1_000))
        .await
        .unwrap();

    assert!(matches!(
        result.outcome,
        SubagentOutcome::Completed {
            output,
            truncated: false,
            ..
        } if output == "AGENT_CHILD_OK"
    ));
}

#[tokio::test]
async fn wait_reduces_a_completed_child_beyond_the_legacy_event_page() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    let identity = talos_agent_api::AgentTurnRef::new(&child.session_id, &turn_id);
    let active = services
        .store
        .get_agent_active_turn(&child.session_id)
        .await
        .unwrap()
        .unwrap();
    let events = services
        .store
        .list_agent_turn_events(&identity, 64)
        .await
        .unwrap();
    let mut expected_event_seq = active.last_event_seq;
    let mut sequence = events.last().unwrap().sequence + 1;

    for _ in 0..260 {
        let committed = services
            .store
            .commit_progress_event(
                CanonicalAgentEvent {
                    identity: identity.clone(),
                    sequence,
                    occurred_at: 2,
                    payload: CanonicalAgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
                },
                expected_event_seq,
            )
            .await
            .unwrap();
        expected_event_seq = committed.active_turn.last_event_seq;
        sequence += 1;
    }
    services
        .store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: CanonicalAgentEvent {
                identity,
                sequence,
                occurred_at: 2,
                payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq,
            completed_turn_metrics: Some(AgentRunMetrics::default()),
        })
        .await
        .unwrap();

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 0))
        .await
        .unwrap();

    assert!(matches!(
        result.outcome,
        SubagentOutcome::Completed {
            truncated: false,
            ..
        }
    ));
}

#[tokio::test]
async fn wait_timeout_returns_the_running_child_as_pending() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _turn_id) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 1))
        .await
        .unwrap();

    assert_eq!(result.subagent, child);
    assert_eq!(result.agent_id, "reviewer");
    assert!(matches!(result.outcome, SubagentOutcome::Running { .. }));
}

#[tokio::test]
async fn wait_resolves_a_relative_direct_child_target() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _) = create_running_child(&services, parent.clone()).await;

    let result = services
        .subagents
        .wait(WaitSubagentRequest {
            caller: parent,
            target: AgentTarget::DirectChild {
                name: child.path.name().to_string(),
            },
            timeout_ms: 0,
        })
        .await
        .unwrap();

    assert_eq!(result.subagent, child);
    assert!(matches!(result.outcome, SubagentOutcome::Running { .. }));
}

#[tokio::test]
async fn wait_pending_snapshot_exposes_bounded_latest_commentary() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    commit_agent_text(
        services.store.as_ref(),
        services.timeline.as_ref(),
        &child.session_id,
        &turn_id,
        "commentary-1",
        AgentAssistantTextPhase::Commentary,
        format!("reviewing\n{}", "x".repeat(3_000)),
    )
    .await;

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 0))
        .await
        .unwrap();
    let update = result.latest_update.as_ref().unwrap();

    assert_eq!(update.kind, talos_types::SubagentUpdateKind::Commentary);
    assert!(update.text.starts_with("reviewing "));
    assert_eq!(update.text.len(), 2 * 1024);
    assert!(update.truncated);
}

#[tokio::test]
async fn failed_outcome_exposes_a_bounded_preview_with_truncation_marker() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    settle_canonical_turn(
        services.store.as_ref(),
        services.timeline.as_ref(),
        &child.session_id,
        &turn_id,
        CanonicalAgentEventPayload::TurnFailed(talos_agent_api::AgentTurnFailed {
            error: talos_agent_api::AgentErrorInfo {
                code: talos_error::ErrorCode::InternalError,
                message: "failed ".repeat(400),
                retryable: false,
            },
        }),
    )
    .await;

    let result = services
        .subagents
        .wait(wait_request(parent, &child, 0))
        .await
        .unwrap();

    assert!(matches!(
        &result.outcome,
        SubagentOutcome::Failed {
            error,
            truncated: true,
            ..
        } if error.len() == 2 * 1024
    ));
}

#[tokio::test]
async fn wait_rejects_a_child_owned_by_another_parent() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, _turn_id) = create_running_child(&services, parent.clone()).await;

    let error = services
        .subagents
        .wait(wait_request(
            AgentCaller {
                session_id: "other-parent".to_string(),
                turn_id: parent.turn_id,
                agent_id: parent.agent_id,
            },
            &child,
            0,
        ))
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn concurrent_waiters_observe_live_completion() {
    let services = test_services().await;
    let parent = create_parent(&services).await;
    let (child, turn_id) = create_running_child(&services, parent.clone()).await;
    let request = WaitSubagentRequest {
        caller: parent,
        target: target(&child),
        timeout_ms: 1_000,
    };
    let timeline = services.timeline.clone();
    let store = services.store.clone();
    let session_id = child.session_id;
    let complete = tokio::spawn(async move {
        tokio::task::yield_now().await;
        commit_agent_text(
            store.as_ref(),
            timeline.as_ref(),
            &session_id,
            &turn_id,
            "message-child-answer",
            AgentAssistantTextPhase::FinalAnswer,
            "AGENT_CHILD_OK".to_string(),
        )
        .await;
        settle_canonical_turn(
            store.as_ref(),
            timeline.as_ref(),
            &session_id,
            &turn_id,
            CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        )
        .await;
    });

    let (first, second) = tokio::join!(
        services.subagents.wait(request.clone()),
        services.subagents.wait(request)
    );
    complete.await.unwrap();

    assert!(matches!(
        first.unwrap().outcome,
        SubagentOutcome::Completed { .. }
    ));
    assert!(matches!(
        second.unwrap().outcome,
        SubagentOutcome::Completed { .. }
    ));
}
