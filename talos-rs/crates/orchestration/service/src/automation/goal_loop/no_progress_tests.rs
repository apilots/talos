use super::*;
use talos_agent_api::{
    AgentMessage, AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStatus, AgentToolCall, AgentToolCallArguments,
};
use talos_store::session::AgentEventCommit;

async fn append_tool_call(
    store: &DatabaseSessionStore,
    identity: &AgentTurnRef,
    mut commit: AgentEventCommit,
    name: &str,
) -> AgentEventCommit {
    let message = AgentMessage {
        message_id: format!("tool-message-{}", commit.active_turn.last_event_seq),
        turn_id: identity.turn_id.clone(),
        role: AgentMessageRole::Agent,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "call-block".into(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::ToolCall(AgentToolCall {
                call_id: if name == "automation" {
                    "call"
                } else {
                    "work-call"
                }
                .into(),
                name: name.into(),
                arguments: AgentToolCallArguments::Complete {
                    value: serde_json::json!({}).into(),
                },
                progress: vec![],
            }),
            started_at: 2,
            completed_at: Some(2),
        }],
        created_at: 2,
        completed_at: Some(2),
    };
    let sequence = commit.events.last().unwrap().sequence + 1;
    for event in
        crate::test_support::canonical_completed_message_events(identity.clone(), message, sequence)
    {
        commit = store
            .commit_progress_event(event, commit.active_turn.last_event_seq)
            .await
            .unwrap();
    }
    commit
}
async fn caller(store: &DatabaseSessionStore, identity: &AgentTurnRef) -> AgentCaller {
    AgentCaller {
        session_id: identity.session_id.clone(),
        turn_id: identity.turn_id.clone(),
        agent_id: store
            .get_session_record(&identity.session_id)
            .await
            .unwrap()
            .unwrap()
            .agent_id,
    }
}
async fn settle_with_tools(
    service: &TalosService,
    store: &DatabaseSessionStore,
    identity: AgentTurnRef,
    commit: AgentEventCommit,
    tool_calls: u32,
) {
    let event = normal_end(store, identity, commit).await;
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    service
        .turns
        .automation_service()
        .unwrap()
        .settle(
            &session,
            &event,
            Some(&AgentRunMetrics {
                tool_calls,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
}
async fn active_goal_turn(
    service: &TalosService,
    store: &DatabaseSessionStore,
) -> (AgentTurnRef, AgentEventCommit) {
    store
        .automation_store()
        .create_goal(&goal("g", "alice", GoalStatus::Active), None)
        .await
        .unwrap();
    admitted(
        service,
        store,
        Some(AutomationTurnOrigin::Goal {
            goal_id: "g".into(),
            objective_revision: 1,
        }),
        crate::test_support::test_user_input("Do Goal work"),
    )
    .await
}
#[tokio::test]
async fn control_only_goal_read_is_no_progress() {
    let (service, store) = fixture().await;
    let (identity, commit) = active_goal_turn(&service, &store).await;
    let commit = append_tool_call(&store, &identity, commit, "automation").await;
    service
        .execute_automation_model(AutomationModelRequest {
            caller: caller(&store, &identity).await,
            call_id: "call".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Read {},
            },
        })
        .await
        .unwrap();
    settle_with_tools(&service, &store, identity, commit, 1).await;
    let result = store.automation_store().goal("g").await.unwrap();
    assert_eq!(result.status, GoalStatus::Paused);
    assert_eq!(result.pause_reason, Some(GoalPauseReason::NoProgress));
}
#[tokio::test]
async fn successful_goal_registration_waits_without_no_progress_pause() {
    let (service, store) = fixture().await;
    let mut input = crate::test_support::test_user_input("Finish tests");
    input.content.push(AgentUserContent::SkillSelection {
        name: "goal".into(),
        path: "/verified-system/goal/SKILL.md".into(),
    });
    let (identity, commit) = admitted(&service, &store, None, input).await;
    let commit = append_tool_call(&store, &identity, commit, "automation").await;
    let result = service
        .execute_automation_model(AutomationModelRequest {
            caller: caller(&store, &identity).await,
            call_id: "call".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Create {
                    objective: "Finish tests".into(),
                    token_budget: None,
                },
            },
        })
        .await
        .unwrap();
    let id = result.snapshot.goal.unwrap().id;
    let registered = store.automation_store().goal(&id).await.unwrap();
    let mut waiting = registered.clone();
    waiting.created_at = chrono::Utc::now().timestamp() + 86_400;
    store
        .automation_store()
        .replace_goal(&registered, &waiting)
        .await
        .unwrap();
    assert!(service
        .execute_automation_model(AutomationModelRequest {
            caller: caller(&store, &identity).await,
            call_id: "premature-complete".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Complete {
                    goal_id: id.clone(),
                    objective_revision: 1,
                    evidence: "No work has run".into(),
                }
            },
        })
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::Conflict));
    settle_with_tools(&service, &store, identity, commit, 1).await;
    assert_eq!(
        store.automation_store().goal(&id).await.unwrap().status,
        GoalStatus::Active
    );
    let session = store.get_session_record("alice").await.unwrap().unwrap();
    service.turns.continue_goal(session, false).await.unwrap();
    assert!(store
        .get_agent_active_turn("alice")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .automation_store()
        .goal(&id)
        .await
        .unwrap()
        .latest_turn_id
        .is_none());
}
#[tokio::test]
async fn ordinary_work_tool_retains_goal_progress() {
    let (service, store) = fixture().await;
    let (identity, commit) = active_goal_turn(&service, &store).await;
    let commit = append_tool_call(&store, &identity, commit, "automation").await;
    let commit = append_tool_call(&store, &identity, commit, "Read").await;
    settle_with_tools(&service, &store, identity, commit, 2).await;
    assert_eq!(
        store.automation_store().goal("g").await.unwrap().status,
        GoalStatus::Active
    );
}
#[tokio::test]
async fn completion_declaration_wins_over_control_only_no_progress() {
    let (service, store) = fixture().await;
    let (identity, commit) = active_goal_turn(&service, &store).await;
    let commit = append_tool_call(&store, &identity, commit, "automation").await;
    service
        .execute_automation_model(AutomationModelRequest {
            caller: caller(&store, &identity).await,
            call_id: "call".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Complete {
                    goal_id: "g".into(),
                    objective_revision: 1,
                    evidence: "All checks passed".into(),
                },
            },
        })
        .await
        .unwrap();
    settle_with_tools(&service, &store, identity, commit, 1).await;
    let result = store.automation_store().goal("g").await.unwrap();
    assert_eq!(result.status, GoalStatus::Complete);
    assert_eq!(
        result.completion_evidence.as_deref(),
        Some("All checks passed")
    );
}
#[tokio::test]
async fn idempotent_goal_create_ack_does_not_earn_another_registration_exception() {
    let (service, store) = fixture().await;
    let (identity, commit) = active_goal_turn(&service, &store).await;
    let commit = append_tool_call(&store, &identity, commit, "automation").await;
    service
        .execute_automation_model(AutomationModelRequest {
            caller: caller(&store, &identity).await,
            call_id: "call".into(),
            action: AutomationAction::Goal {
                action: GoalAction::Create {
                    objective: "Finish migration".into(),
                    token_budget: None,
                },
            },
        })
        .await
        .unwrap();
    settle_with_tools(&service, &store, identity, commit, 1).await;
    assert_eq!(
        store
            .automation_store()
            .goal("g")
            .await
            .unwrap()
            .pause_reason,
        Some(GoalPauseReason::NoProgress)
    );
}
