use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use crate::session::{AgentExecutionStore, SessionRecord, SessionStore, SessionTreeIdentity};
use std::sync::Arc;
use talos_agent_api::{
    AgentEvent, AgentMessage, AgentMessageRole, AgentMessageStatus, AgentRunMetrics,
    AgentStopReason, AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
};
use talos_storage::SqliteStorage;
use talos_types::talos::{SessionSourceProvider, SessionSourceType};
use talos_types::{SessionConf, SessionUsage};

async fn store() -> (DatabaseSessionStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        SqliteStorage::new_with_max_connections(
            dir.path().join("projection.db").to_str().unwrap(),
            1,
        )
        .await
        .unwrap(),
    );
    run_migrations(db.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(db).await.unwrap();
    store
        .insert_session_record(&session_record())
        .await
        .unwrap();
    (store, dir)
}
fn session_record() -> SessionRecord {
    SessionRecord {
        session_kind: talos_types::SessionKind::Standard,
        id: "session-1".to_string(),
        agent_id: "default".to_string(),
        execution_node_id: "embedded".to_string(),
        tree_identity: SessionTreeIdentity::Primary,
        workspace_id: "default".to_string(),
        title: "Agent execution".to_string(),
        user_id: "user-1".to_string(),
        source_type: SessionSourceType::System,
        source_provider: SessionSourceProvider::System,
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
        worktree_binding: None,
        current_directory: None,
        pinned: false,
    }
}

fn fact(offset: i64, sequence: u64, payload: AgentEventPayload) -> AgentCommittedEvent {
    let settlement = payload
        .is_terminal()
        .then(|| AgentExecutionSettlement::Finished {
            memory_usage: vec![],
            metrics: AgentRunMetrics::default(),
        });
    AgentCommittedEvent {
        execution_purpose: agent_protocol::AgentExecutionPurpose::Turn,
        history_supplement: None,
        cursor: AgentSessionEventCursor {
            session_id: "session-1".into(),
            generation: 0,
            offset,
        },
        event: AgentEvent {
            identity: AgentTurnRef::new("session-1", "turn-1"),
            sequence,
            occurred_at: 10,
            payload,
        },
        settlement,
        automation_input: None,
        command: None,
        execution_model: None,
    }
}
fn admission() -> Vec<AgentCommittedEvent> {
    vec![
        fact(4, 1, AgentEventPayload::TurnStarted(AgentTurnStarted {})),
        fact(
            9,
            2,
            AgentEventPayload::MessageCompleted(AgentMessage {
                message_id: "message-1".into(),
                turn_id: "turn-1".into(),
                role: AgentMessageRole::User,
                status: AgentMessageStatus::Completed,
                blocks: vec![],
                created_at: 10,
                completed_at: Some(10),
            }),
        ),
    ]
}
fn params(events: Vec<AgentCommittedEvent>) -> WorkerProjectionParams {
    WorkerProjectionParams {
        execution_node_id: "embedded".into(),
        session_id: "session-1".into(),
        after_cursor: None,
        page: AgentSessionEventsResult {
            next_cursor: events.last().unwrap().cursor.clone(),
            events,
            has_more: false,
            active_turn: None,
        },
    }
}
#[tokio::test]
async fn cursor_and_facts_commit_together_and_replay_deduplicates() {
    let (s, _dir) = store().await;
    let first = s.project_worker_events(params(admission())).await.unwrap();
    assert_eq!(first.events.len(), 2);
    assert_eq!(first.cursor.offset, 9);
    assert_eq!(
        first.active_turn.unwrap().initial_message_id.as_deref(),
        Some("message-1")
    );
    assert!(s
        .project_worker_events(params(admission()))
        .await
        .unwrap()
        .events
        .is_empty());
    assert_eq!(
        s.worker_event_cursor("session-1")
            .await
            .unwrap()
            .unwrap()
            .offset,
        9
    );
}
#[tokio::test]
async fn invalid_tail_rolls_back_admission_and_cursor() {
    let (s, _dir) = store().await;
    let mut events = admission();
    events.push(fact(
        15,
        4,
        AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
    ));
    assert!(s.project_worker_events(params(events)).await.is_err());
    assert!(s.worker_event_cursor("session-1").await.unwrap().is_none());
    assert!(s
        .get_agent_active_turn("session-1")
        .await
        .unwrap()
        .is_none());
    assert!(s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn changed_source_fact_is_rejected_without_advancing_cursor() {
    let (s, _dir) = store().await;
    s.project_worker_events(params(admission())).await.unwrap();
    let mut changed = admission();
    changed[1].event.occurred_at = 11;
    assert!(s.project_worker_events(params(changed)).await.is_err());
    assert_eq!(
        s.worker_event_cursor("session-1")
            .await
            .unwrap()
            .unwrap()
            .offset,
        9
    );
}
#[tokio::test]
async fn terminal_settlement_is_recoverable_until_applied() {
    let (s, _dir) = store().await;
    let mut events = admission();
    events.push(fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    ));
    s.project_worker_events(params(events.clone()))
        .await
        .unwrap();
    assert!(s
        .get_agent_active_turn("session-1")
        .await
        .unwrap()
        .is_none());
    let pending = s.pending_worker_settlements("session-1").await.unwrap();
    assert_eq!(pending.len(), 1);
    s.complete_worker_settlement(&pending[0].cursor)
        .await
        .unwrap();
    s.project_worker_events(params(events)).await.unwrap();
    assert!(s
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn stale_generation_cannot_restore_cleared_history() {
    let (s, _dir) = store().await;
    s.clear_session_history("session-1", 20).await.unwrap();
    assert!(s.project_worker_events(params(admission())).await.is_err());
    assert!(s.worker_event_cursor("session-1").await.unwrap().is_none());
}
#[tokio::test]
async fn another_worker_cannot_project_into_bound_session() {
    let (s, _dir) = store().await;
    let mut p = params(admission());
    p.execution_node_id = "foreign-worker".into();
    assert!(s.project_worker_events(p).await.is_err());
    assert!(s.worker_event_cursor("session-1").await.unwrap().is_none());
}

#[tokio::test]
async fn clearing_resets_projection_source_and_keeps_old_settlements_hidden() {
    let (s, _dir) = store().await;
    let mut old = admission();
    old.push(fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    ));
    s.project_worker_events(params(old)).await.unwrap();
    let receipt = s.pending_worker_settlements("session-1").await.unwrap();
    s.complete_worker_settlement(&receipt[0].cursor)
        .await
        .unwrap();
    s.clear_session_history("session-1", 20).await.unwrap();
    assert!(s
        .pending_worker_settlements("session-1")
        .await
        .unwrap()
        .is_empty());
    let mut new = admission();
    for f in &mut new {
        f.cursor.generation = 1;
        f.event.identity.turn_id = "turn-2".into();
        if let AgentEventPayload::MessageCompleted(m) = &mut f.event.payload {
            m.turn_id = "turn-2".into();
        }
    }
    let commit = s.project_worker_events(params(new)).await.unwrap();
    assert_eq!(commit.cursor.generation, 1);
    assert_eq!(commit.events.len(), 2);
}

#[tokio::test]
async fn destructive_store_operations_retain_unapplied_settlements() {
    enum Operation {
        Clear,
        Delete,
        DeleteTree,
    }
    for operation in [Operation::Clear, Operation::Delete, Operation::DeleteTree] {
        let (s, _dir) = store().await;
        let mut events = admission();
        events.push(fact(
            14,
            3,
            AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            }),
        ));
        s.project_worker_events(params(events)).await.unwrap();
        let before = s.pending_worker_settlements("session-1").await.unwrap();
        assert_eq!(before.len(), 1);
        let error = match operation {
            Operation::Clear => s.clear_session_history("session-1", 20).await,
            Operation::Delete => s.delete_session_record("session-1").await,
            Operation::DeleteTree => s.delete_session_tree("session-1").await,
        }
        .unwrap_err();
        assert!(error.is_code(ErrorCode::Conflict));
        assert!(s.get_session_record("session-1").await.unwrap().is_some());
        let retained = s.pending_worker_settlements("session-1").await.unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].cursor, before[0].cursor);
        s.complete_worker_settlement(&retained[0].cursor)
            .await
            .unwrap();
        assert!(match operation {
            Operation::Clear => s.clear_session_history("session-1", 20).await,
            Operation::Delete => s.delete_session_record("session-1").await,
            Operation::DeleteTree => s.delete_session_tree("session-1").await,
        }
        .unwrap());
    }
}

#[tokio::test]
async fn deleting_a_tree_preserves_a_descendants_pending_settlement() {
    let (s, _dir) = store().await;
    let mut child = session_record();
    child.id = "child".into();
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: AgentTurnRef::new("session-1", "parent-turn"),
        tree_root_session_id: "session-1".into(),
        path: "/root/child".parse().unwrap(),
    };
    s.insert_session_record(&child).await.unwrap();
    let mut events = admission();
    events.push(fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    ));
    for event in &mut events {
        event.cursor.session_id = child.id.clone();
        event.event.identity.session_id = child.id.clone();
    }
    let mut projection = params(events);
    projection.session_id = child.id.clone();
    s.project_worker_events(projection).await.unwrap();
    assert!(s
        .delete_session_tree("session-1")
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    for id in ["session-1", "child"] {
        assert!(s.get_session_record(id).await.unwrap().is_some());
    }
    let receipt = s.pending_worker_settlements("child").await.unwrap();
    assert_eq!(receipt.len(), 1);
    s.complete_worker_settlement(&receipt[0].cursor)
        .await
        .unwrap();
    assert!(s.delete_session_tree("session-1").await.unwrap());
}

#[tokio::test]
async fn terminal_trajectory_metadata_commits_with_cursor_and_replays_once() {
    use talos_agent_api::trajectory::{ClockRelation, ClockSegment, SessionHistorySupplement};
    let (s, _dir) = store().await;
    let mut events = admission();
    let mut terminal = fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );
    let supplement = SessionHistorySupplement {
        identity: terminal.event.identity.clone(),
        clock_segments: vec![ClockSegment {
            segment_id: "execution-clock".into(),
            source_node_id: "embedded".into(),
            started_at: 10,
            relation: ClockRelation::TurnStart,
            observed_through_offset_ms: 25.0,
        }],
        requests: vec![],
        tools: vec![],
        payloads: Default::default(),
    };
    terminal.history_supplement = Some(supplement.clone());
    events.push(terminal);
    s.project_worker_events(params(events.clone()))
        .await
        .unwrap();
    s.project_worker_events(params(events)).await.unwrap();
    assert_eq!(
        s.read_terminal_history_supplement(&supplement.identity)
            .await
            .unwrap(),
        Some(supplement)
    );
    assert_eq!(
        s.worker_event_cursor("session-1")
            .await
            .unwrap()
            .unwrap()
            .offset,
        14
    );
}

#[tokio::test]
async fn foreign_trajectory_metadata_rolls_back_the_whole_page() {
    use talos_agent_api::trajectory::SessionHistorySupplement;
    let (s, _dir) = store().await;
    let mut events = admission();
    let mut terminal = fact(
        14,
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );
    terminal.history_supplement = Some(SessionHistorySupplement {
        identity: AgentTurnRef::new("foreign-session", "turn-1"),
        clock_segments: vec![],
        requests: vec![],
        tools: vec![],
        payloads: Default::default(),
    });
    events.push(terminal);
    assert!(s.project_worker_events(params(events)).await.is_err());
    assert!(s.worker_event_cursor("session-1").await.unwrap().is_none());
    assert!(s
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn compaction_usage_and_original_purpose_project_once_without_counting_a_turn() {
    let (s, _dir) = store().await;
    let mut started = fact(4, 1, AgentEventPayload::TurnStarted(AgentTurnStarted {}));
    started.execution_purpose = agent_protocol::AgentExecutionPurpose::Compaction;
    let mut terminal = fact(
        9,
        2,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );
    terminal.execution_purpose = agent_protocol::AgentExecutionPurpose::Compaction;
    let Some(AgentExecutionSettlement::Finished { metrics, .. }) = &mut terminal.settlement else {
        panic!("Terminal fixture has no settlement")
    };
    metrics.usage = talos_types::TokenUsage {
        total_tokens: 23,
        ..Default::default()
    };
    let events = vec![started, terminal];
    s.project_worker_events(params(events.clone()))
        .await
        .unwrap();
    s.project_worker_events(params(events)).await.unwrap();
    let session = s.get_session_record("session-1").await.unwrap().unwrap();
    assert_eq!(session.usage.tokens.total_tokens, 23);
    assert_eq!(session.message_count, 0);
    assert_eq!(session.latest_turn_seq, 0);
    assert_eq!(
        s.pending_worker_settlements("session-1").await.unwrap()[0].execution_purpose,
        agent_protocol::AgentExecutionPurpose::Compaction
    );
}

#[path = "worker_settlement_notice_tests.rs"]
mod notice_tests;

#[path = "worker_title_tests.rs"]
mod worker_title_tests;

#[path = "worker_projection_read_tests.rs"]
mod read_tests;
