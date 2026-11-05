use super::*;
use crate::session::{
    AgentExecutionStore, CommitInitialAdmissionParams, DatabaseSessionStore, SessionRecord,
    SessionStore, SessionTreeIdentity,
};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole, AgentMessageStatus,
    AgentTurnStarted,
};
use talos_types::{LoopSchedule, SessionConf, SessionUsage};
struct Fixture {
    _dir: tempfile::TempDir,
    storage: Arc<talos_storage::SqliteStorage>,
    session: DatabaseSessionStore,
    automation: DatabaseAutomationStore,
}
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        talos_storage::SqliteStorage::new(dir.path().join("automation.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    crate::migration::run_migrations(storage.pool(), &crate::migration::test_migrations_dir())
        .await
        .unwrap();
    let session = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    session
        .insert_session_record(&SessionRecord {
            id: "s".into(),
            agent_id: "default".into(),
            execution_node_id: "embedded".into(),
            tree_identity: SessionTreeIdentity::Primary,
            workspace_id: "default".into(),
            title: "Automation".into(),
            user_id: "u".into(),
            source_type: talos_types::talos::SessionSourceType::System,
            source_provider: talos_types::talos::SessionSourceProvider::System,
            channel_id: None,
            thread_id: None,
            environment: None,
            created_at: 1,
            last_activity: 1,
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
            usage: SessionUsage::default(),
            conf: SessionConf::default(),
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    Fixture {
        _dir: dir,
        automation: DatabaseAutomationStore::new(storage.clone()),
        session,
        storage,
    }
}
fn goal(id: &str) -> SessionGoal {
    SessionGoal {
        id: id.into(),
        session_id: "s".into(),
        objective: "Pass checks".into(),
        objective_revision: 1,
        status: GoalStatus::Active,
        pause_reason: None,
        reason: None,
        token_budget: Some(100),
        tokens_used: 0,
        latest_turn_id: None,
        completion_evidence: None,
        created_at: 1,
        updated_at: 1,
    }
}
fn task() -> SessionLoop {
    SessionLoop {
        id: "l".into(),
        session_id: "s".into(),
        prompt: "Check CI".into(),
        schedule: LoopSchedule::ModelDecided {},
        config_revision: 1,
        status: LoopStatus::Active,
        stop_reason: None,
        reason: None,
        next_due_at: None,
        latest_turn_id: None,
        created_at: 1,
        updated_at: 1,
    }
}
async fn admit(f: &Fixture, origin: Option<AutomationTurnOrigin>) -> TalosResult<()> {
    let identity = AgentTurnRef::new("s", "t");
    f.session
        .commit_initial_admission(CommitInitialAdmissionParams {
            automation_origin: origin,
            command: None,
            events: vec![
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 1,
                    occurred_at: 1,
                    payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
                },
                AgentEvent {
                    identity,
                    sequence: 2,
                    occurred_at: 1,
                    payload: AgentEventPayload::MessageCompleted(AgentMessage {
                        message_id: "m".into(),
                        turn_id: "t".into(),
                        role: AgentMessageRole::User,
                        status: AgentMessageStatus::Completed,
                        blocks: vec![],
                        created_at: 1,
                        completed_at: Some(1),
                    }),
                },
            ],
        })
        .await
        .map(|_| ())
}
#[tokio::test]
async fn origin_and_cumulative_usage_are_bound_to_the_original_goal() {
    let f = fixture().await;
    admit(&f, None).await.unwrap();
    let g = goal("old");
    let identity = AgentTurnRef::new("s", "t");
    f.automation.create_goal(&g, Some(&identity)).await.unwrap();
    f.automation.account_usage(&identity, 10, 2).await.unwrap();
    f.automation.account_usage(&identity, 10, 2).await.unwrap();
    f.automation.account_usage(&identity, 7, 2).await.unwrap();
    let previous = f.automation.goal("old").await.unwrap();
    let mut cleared = previous.clone();
    cleared.status = GoalStatus::Cleared;
    f.automation
        .replace_goal(&previous, &cleared)
        .await
        .unwrap();
    f.automation.create_goal(&goal("new"), None).await.unwrap();
    f.automation.account_usage(&identity, 16, 3).await.unwrap();
    assert_eq!(f.automation.goal("old").await.unwrap().tokens_used, 16);
    assert_eq!(f.automation.goal("new").await.unwrap().tokens_used, 0);
    assert_eq!(
        f.automation.origin(&identity).await.unwrap(),
        Some(AutomationTurnOrigin::Goal {
            goal_id: "old".into(),
            objective_revision: 1
        })
    );
}
#[tokio::test]
async fn clear_loop_retains_admitted_history_provenance() {
    let f = fixture().await;
    let l = task();
    f.automation.create_loop(&l).await.unwrap();
    let origin = AutomationTurnOrigin::Loop {
        loop_id: l.id.clone(),
        occurrence_id: "o".into(),
        config_revision: 1,
    };
    admit(&f, Some(origin.clone())).await.unwrap();
    let admitted = f.automation.loop_task(&l.id).await.unwrap();
    f.automation.clear_loop(&admitted).await.unwrap();
    assert_eq!(
        f.automation
            .origin(&AgentTurnRef::new("s", "t"))
            .await
            .unwrap(),
        Some(origin)
    );
}
#[tokio::test]
async fn later_user_steer_does_not_inherit_triggering_input_origin() {
    let f = fixture().await;
    admit(&f, None).await.unwrap();
    let identity = AgentTurnRef::new("s", "t");
    f.automation
        .create_goal(&goal("g"), Some(&identity))
        .await
        .unwrap();
    let active = f.session.get_agent_active_turn("s").await.unwrap().unwrap();
    f.session
        .commit_running_input_admission(crate::session::CommitRunningInputAdmissionParams {
            expected_event_seq: active.last_event_seq,
            command: None,
            events: vec![AgentEvent {
                identity: identity.clone(),
                sequence: 3,
                occurred_at: 2,
                payload: AgentEventPayload::MessageCompleted(AgentMessage {
                    message_id: "steer-message".into(),
                    turn_id: "t".into(),
                    role: AgentMessageRole::User,
                    status: AgentMessageStatus::Completed,
                    blocks: vec![],
                    created_at: 2,
                    completed_at: Some(2),
                }),
            }],
        })
        .await
        .unwrap();
    let binding = f.automation.input_origin(&identity).await.unwrap().unwrap();
    assert_eq!(binding.message_id, "m");
    assert_ne!(binding.message_id, "steer-message");
    let batch = f
        .automation
        .input_origins(&[identity.clone(), AgentTurnRef::new("s", "ordinary")])
        .await
        .unwrap();
    assert_eq!(batch, vec![(identity, binding)]);
}
#[tokio::test]
async fn tool_call_identity_allows_multiple_loops_without_duplicate_or_clear_revival() {
    let f = fixture().await;
    admit(&f, None).await.unwrap();
    let identity = AgentTurnRef::new("s", "t");
    let first = task();
    let stored = f
        .automation
        .create_loop_for_call(&first, &identity, "call-1")
        .await
        .unwrap();
    assert_eq!(stored, first);
    let mut retry = task();
    retry.id = "discarded-proposal".into();
    assert_eq!(
        f.automation
            .create_loop_for_call(&retry, &identity, "call-1")
            .await
            .unwrap(),
        first
    );
    let mut second = task();
    second.id = "second".into();
    f.automation
        .create_loop_for_call(&second, &identity, "call-2")
        .await
        .unwrap();
    assert_eq!(f.automation.loops("s").await.unwrap().len(), 2);
    f.automation.clear_loop(&stored).await.unwrap();
    let late = f
        .automation
        .create_loop_for_call(&retry, &identity, "call-1")
        .await
        .unwrap();
    assert_eq!(late.id, "l");
    assert_eq!(late.status, LoopStatus::Stopped);
    assert!(f.automation.loop_task("l").await.is_err());
    assert_eq!(f.automation.loops("s").await.unwrap(), vec![second]);
}
#[tokio::test]
async fn original_session_deletion_cleans_definitions_without_foreign_key_assumptions() {
    let f = fixture().await;
    f.automation.create_loop(&task()).await.unwrap();
    f.storage
        .execute("DELETE FROM sessions WHERE session_key='s'", &[])
        .await
        .unwrap();
    assert!(f.automation.loop_task("l").await.is_err());
}
