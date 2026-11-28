use crate::test_support::{ProjectTestWorkerEvents, TestWorkerEvents};
use std::sync::Arc;
use std::{ops::Deref, path::PathBuf};

use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole, AgentMessageStatus,
    AgentPendingInput, AgentRunMetrics, AgentStopReason, AgentTurnCompleted, AgentTurnRef,
    AgentTurnStarted,
};
use talos_storage::SqliteStorage;
use talos_types::{ContextUsage, TokenUsage};

use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use crate::session::{
    AccumulateSessionUsageParams, DatabaseSessionStore, SessionRecord, SessionStore,
    WorkerProjectionParams,
};

#[path = "agent_execution_cancellation_tests.rs"]
mod cancellation_tests;
#[path = "agent_execution_idempotency_tests.rs"]
mod idempotency_tests;

struct TestStore {
    store: DatabaseSessionStore,
    _directory: tempfile::TempDir,
}

impl Deref for TestStore {
    type Target = DatabaseSessionStore;

    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

async fn store() -> TestStore {
    store_with_record(session_record()).await
}

async fn store_with_record(record: SessionRecord) -> TestStore {
    let directory = tempfile::tempdir().unwrap();
    let database_path: PathBuf = directory.path().join("agent-execution.db");
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(database_path.to_str().unwrap(), 1)
            .await
            .unwrap(),
    );
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage).await.unwrap();
    store.insert_session_record(&record).await.unwrap();
    TestStore {
        store,
        _directory: directory,
    }
}

fn event(sequence: u64, payload: AgentEventPayload) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        sequence,
        occurred_at: 10,
        payload,
    }
}

fn admission() -> Vec<AgentEvent> {
    admission_for("session-1", "turn-1", "message-1")
}

fn initial_admission(events: Vec<AgentEvent>) -> TestWorkerEvents {
    TestWorkerEvents::new(events)
}

fn admission_for(session_id: &str, turn_id: &str, message_id: &str) -> Vec<AgentEvent> {
    vec![
        AgentEvent {
            identity: AgentTurnRef::new(session_id, turn_id),
            sequence: 1,
            occurred_at: 10,
            payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        },
        AgentEvent {
            identity: AgentTurnRef::new(session_id, turn_id),
            sequence: 2,
            occurred_at: 10,
            payload: AgentEventPayload::MessageCompleted(AgentMessage {
                message_id: message_id.to_string(),
                turn_id: turn_id.to_string(),
                role: AgentMessageRole::User,
                status: AgentMessageStatus::Completed,
                blocks: Vec::new(),
                created_at: 10,
                completed_at: Some(10),
            }),
        },
    ]
}

fn user_admission(sequence: u64, message_id: &str) -> Vec<AgentEvent> {
    vec![event(
        sequence,
        AgentEventPayload::MessageCompleted(AgentMessage {
            message_id: message_id.to_string(),
            turn_id: "turn-1".to_string(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            blocks: Vec::new(),
            created_at: 10,
            completed_at: Some(10),
        }),
    )]
}

fn waiting(sequence: u64, action_id: &str) -> AgentEvent {
    event(
        sequence,
        AgentEventPayload::WaitingForInput(AgentPendingInput::Message {
            action_id: action_id.to_string(),
            prompt: "Continue?".to_string(),
        }),
    )
}

fn session_record() -> SessionRecord {
    let mut record = crate::test_support::test_session_record("session-1");
    record.title = "Agent execution".into();
    record
}

#[tokio::test]
async fn failed_initial_admission_rolls_back_its_event_batch() {
    let store = store().await;
    store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();

    let second_identity = AgentTurnRef::new("session-1", "turn-2");
    assert!(store
        .project_test_events(initial_admission(admission_for(
            "session-1",
            "turn-2",
            "message-2"
        )))
        .await
        .is_err());

    assert!(store
        .list_agent_turn_events(&second_identity, 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn initial_admission_atomically_creates_events_and_active_index() {
    let store = store().await;

    let commit = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();

    assert_eq!(commit.events.len(), 2);
    assert_eq!(
        commit
            .active_turn
            .as_ref()
            .unwrap()
            .initial_message_id
            .as_deref(),
        Some("message-1")
    );
    assert_eq!(commit.active_turn.as_ref().unwrap().last_event_seq, 2);
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        commit.active_turn
    );
}

#[tokio::test]
async fn maintenance_admission_does_not_fabricate_an_initial_message() {
    let store = store().await;

    let commit = store
        .project_test_events(TestWorkerEvents {
            purpose: agent_protocol::AgentExecutionPurpose::Compaction,
            ..TestWorkerEvents::new(vec![event(
                1,
                AgentEventPayload::TurnStarted(AgentTurnStarted {}),
            )])
        })
        .await
        .unwrap();

    assert_eq!(
        commit.active_turn.as_ref().unwrap().initial_message_id,
        None
    );
}

#[tokio::test]
async fn progress_projection_requires_the_exact_source_facts() {
    let store = store().await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let progress = event(
        3,
        AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
    );

    let stale_cursor = Some(agent_protocol::AgentSessionEventCursor {
        session_id: "session-1".into(),
        generation: 0,
        offset: 0,
    });
    assert!(store
        .project_test_events(TestWorkerEvents {
            after_cursor: Some(stale_cursor),
            ..TestWorkerEvents::new(vec![progress.clone()])
        })
        .await
        .is_err());
    let commit = store
        .project_test_events(TestWorkerEvents::new(vec![progress]))
        .await
        .unwrap();

    assert_eq!(commit.events[0].sequence, 3);
    assert_eq!(commit.active_turn.as_ref().unwrap().last_event_seq, 3);
    assert_eq!(
        store
            .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
            .await
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn progress_batch_rejects_gaps_atomically_and_preserves_input_admission_order() {
    let store = store().await;
    let admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let first = event(
        3,
        AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
    );
    let next = event(
        4,
        AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
    );
    let gap = event(
        5,
        AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
    );
    let _tail = admission.active_turn.as_ref().unwrap().last_event_seq;
    assert!(store
        .project_test_events(TestWorkerEvents::new(vec![first.clone(), gap]))
        .await
        .is_err());
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        admission.active_turn
    );
    let commit = store
        .project_test_events(TestWorkerEvents::new(vec![first.clone(), next.clone()]))
        .await
        .unwrap();
    assert_eq!(
        commit
            .events
            .iter()
            .map(|record| AgentEvent {
                identity: record.identity.clone(),
                sequence: record.sequence,
                occurred_at: record.occurred_at,
                payload: record.payload.clone()
            })
            .collect::<Vec<_>>(),
        vec![first, next]
    );
    let input = store
        .project_test_events(TestWorkerEvents::new(user_admission(5, "message-2")))
        .await
        .unwrap();
    assert_eq!(input.events.first().unwrap().sequence, 5);
    assert_eq!(
        store
            .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
            .await
            .unwrap()
            .len(),
        5
    );
}

#[tokio::test]
async fn running_input_admission_advances_the_same_active_turn() {
    let store = store().await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();

    let commit = store
        .project_test_events(TestWorkerEvents::new(user_admission(3, "message-2")))
        .await
        .unwrap();

    assert_eq!(
        (
            commit.events[0].sequence,
            commit
                .active_turn
                .as_ref()
                .unwrap()
                .initial_message_id
                .as_deref(),
            commit.active_turn.as_ref().unwrap().state,
        ),
        (3, Some("message-1"), ActiveTurnState::Running)
    );
}

#[tokio::test]
async fn waiting_input_projection_replays_the_same_worker_facts() {
    let store = store().await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let _waiting = store
        .project_test_events(TestWorkerEvents::new(vec![waiting(3, "action-1")]))
        .await
        .unwrap();
    let events = user_admission(4, "message-2");

    let source = store.worker_event_cursor("session-1").await.unwrap();
    let first = store
        .project_test_events(TestWorkerEvents::new(events.clone()))
        .await
        .unwrap();
    let reconstructed_events = events;
    let retry = store
        .project_test_events(TestWorkerEvents {
            after_cursor: Some(source),
            ..TestWorkerEvents::new(reconstructed_events)
        })
        .await
        .unwrap();

    assert!(retry.events.is_empty());
    assert_eq!(retry.active_turn, first.active_turn);
    assert_eq!(
        store
            .list_agent_turn_events(&first.active_turn.as_ref().unwrap().identity, 10)
            .await
            .unwrap()
            .len(),
        4
    );
}

#[tokio::test]
async fn later_waiting_replaces_the_claim_baseline_after_progress() {
    let store = store().await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let _first_waiting = store
        .project_test_events(TestWorkerEvents::new(vec![waiting(3, "action-1")]))
        .await
        .unwrap();
    let _claim = store
        .project_test_events(TestWorkerEvents::new(user_admission(4, "message-2")))
        .await
        .unwrap();
    let _progress = store
        .project_test_events(TestWorkerEvents::new(vec![event(
            5,
            AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
        )]))
        .await
        .unwrap();

    let second_waiting = store
        .project_test_events(TestWorkerEvents::new(vec![waiting(6, "action-2")]))
        .await
        .unwrap();
    let active = store
        .get_agent_active_turn("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            active.state,
            active.current_action_id.as_deref(),
            active.last_event_seq
        ),
        (
            ActiveTurnState::WaitingForInput,
            Some("action-2"),
            second_waiting.active_turn.as_ref().unwrap().last_event_seq
        )
    );
}

#[tokio::test]
async fn changed_worker_input_facts_cannot_append_a_duplicate() {
    let store = store().await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let _waiting = store
        .project_test_events(TestWorkerEvents::new(vec![waiting(3, "action-1")]))
        .await
        .unwrap();
    let events = user_admission(4, "message-2");
    let source = store.worker_event_cursor("session-1").await.unwrap();
    store
        .project_test_events(TestWorkerEvents::new(events.clone()))
        .await
        .unwrap();

    let mut changed = events;
    changed[0].occurred_at += 1;
    let conflict = store
        .project_test_events(TestWorkerEvents {
            after_cursor: Some(source),
            ..TestWorkerEvents::new(changed)
        })
        .await;
    let event_count = store
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap()
        .len();

    assert_eq!((conflict.is_err(), event_count), (true, 4));
}

#[tokio::test]
async fn terminal_settlement_removes_nonterminal_indexes() {
    let mut original = session_record();
    original.usage.context = ContextUsage::new(7, 100, 80, 1);
    let store = store_with_record(original.clone()).await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let _waiting = store
        .project_test_events(TestWorkerEvents::new(vec![waiting(3, "action-1")]))
        .await
        .unwrap();

    let listed_ids = ["session-1".into()];
    assert!(store
        .active_session_ids(&listed_ids)
        .await
        .unwrap()
        .contains("session-1"));

    let terminal = store
        .project_test_events(TestWorkerEvents {
            metrics: Some(AgentRunMetrics::default()),
            ..TestWorkerEvents::new(vec![event(
                4,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            )])
        })
        .await
        .map(|commit| {
            commit
                .events
                .into_iter()
                .last()
                .expect("terminal Worker projection")
        })
        .unwrap();
    assert!(store
        .active_session_ids(&listed_ids)
        .await
        .unwrap()
        .is_empty());
    let _identity = AgentTurnRef::new("session-1", "turn-1");
    assert_eq!(
        store
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap()
            .usage
            .context,
        original.usage.context,
    );

    assert_eq!(
        (
            terminal.sequence,
            store
                .get_latest_agent_terminal_event("session-1")
                .await
                .unwrap()
                .map(|event| event.payload),
            store
                .get_agent_terminal_event_by_initial_message("session-1", "message-1")
                .await
                .unwrap()
                .map(|event| event.payload),
            store.get_agent_active_turn("session-1").await.unwrap(),
        ),
        (
            4,
            Some(AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            })),
            Some(AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                stop_reason: AgentStopReason::EndTurn,
            })),
            None,
        )
    );
}

#[tokio::test]
async fn terminal_settlement_and_compaction_usage_update_disjoint_session_state() {
    let mut original = session_record();
    original.usage.context = ContextUsage::new(7, 100, 80, 1);
    let store = store_with_record(original.clone()).await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    assert_eq!(
        store
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap()
            .usage
            .context,
        original.usage.context,
    );
    let metrics = AgentRunMetrics {
        usage: TokenUsage {
            input_tokens: 3,
            output_tokens: 2,
            reasoning_tokens: None,
            total_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        context: ContextUsage::new(9, 32, 24, 10),
        ..AgentRunMetrics::default()
    };

    store
        .project_test_events(TestWorkerEvents {
            metrics: Some(metrics.clone()),
            ..TestWorkerEvents::new(vec![event(
                3,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            )])
        })
        .await
        .map(|commit| {
            commit
                .events
                .into_iter()
                .last()
                .expect("terminal Worker projection")
        })
        .unwrap();
    let compaction_usage = TokenUsage {
        input_tokens: 1,
        output_tokens: 1,
        reasoning_tokens: None,
        total_tokens: 2,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: None,
    };
    store
        .accumulate_session_usage(AccumulateSessionUsageParams {
            session_id: "session-1".to_string(),
            tokens: compaction_usage,
            last_activity: 20,
        })
        .await
        .unwrap();

    let session = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            session.message_count,
            session.latest_turn_seq,
            session.usage.tokens,
            session.usage.context,
            store.get_agent_active_turn("session-1").await.unwrap(),
        ),
        (
            2,
            1,
            TokenUsage {
                input_tokens: 4,
                output_tokens: 3,
                reasoning_tokens: None,
                total_tokens: 7,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            metrics.context,
            None,
        )
    );
}

#[tokio::test]
async fn session_clear_hides_history_retains_pending_facts_and_resets_summary() {
    let original = session_record();
    let store = store_with_record(original.clone()).await;
    let reservation = store
        .reserve_command(CommandReservation {
            session_id: "session-1".to_string(),
            command: IdempotentCommand::TurnSubmission,
            idempotency_key: "command-1".to_string(),
            request_hash: "hash-1".to_string(),
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            message_id: "message-1".to_string(),
            lease_id: "lease-1".to_string(),
            now: 10,
            lease_expires_at: 20,
            record_expires_at: Some(100),
        })
        .await
        .unwrap();
    let IdempotencyReservation::Reserved(lease) = reservation else {
        panic!("fresh Session command must reserve its idempotency key");
    };
    let _admission = store
        .project_test_events(TestWorkerEvents {
            command: Some(lease),
            ..TestWorkerEvents::new(admission())
        })
        .await
        .unwrap();
    let metrics = AgentRunMetrics {
        usage: TokenUsage {
            input_tokens: 3,
            output_tokens: 2,
            reasoning_tokens: None,
            total_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        context: ContextUsage::new(9, 32, 24, 10),
        ..AgentRunMetrics::default()
    };
    let terminal = store
        .project_test_events(TestWorkerEvents {
            metrics: Some(metrics),
            ..TestWorkerEvents::new(vec![event(
                3,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            )])
        })
        .await
        .map(|commit| {
            commit
                .events
                .into_iter()
                .last()
                .expect("terminal Worker projection")
        })
        .unwrap();
    store
        .advance_memory_processed_until_event_seq(
            "session-1",
            0,
            terminal.event_seq.try_into().unwrap(),
        )
        .await
        .unwrap();

    let revisions = || async {
        store.storage.query(
            "SELECT COUNT(*) AS count FROM session_command_revisions WHERE session_id='session-1'",
            &[],
        ).await.unwrap()[0].get_i64("count").unwrap()
    };
    assert_eq!(revisions().await, 1);
    for receipt in store.pending_worker_settlements("session-1").await.unwrap() {
        store
            .complete_worker_settlement(&receipt.cursor)
            .await
            .unwrap();
    }
    assert!(store.clear_session_history("session-1", 200).await.unwrap());
    assert_eq!(
        revisions().await,
        1,
        "uncaptured command facts remain durable"
    );

    let mut expected = original;
    expected.title = talos_types::DEFAULT_SESSION_TITLE.to_string();
    expected.last_activity = 200;
    let cleared = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    let events = store
        .list_agent_events_after("session-1", None, 10)
        .await
        .unwrap();
    assert_eq!(cleared, expected);
    assert!(events.is_empty());
    assert_eq!(
        store
            .get_latest_agent_terminal_event("session-1")
            .await
            .unwrap(),
        None
    );
    assert!(matches!(
        store
            .reserve_command(CommandReservation {
                session_id: "session-1".to_string(),
                command: IdempotentCommand::TurnSubmission,
                idempotency_key: "command-1".to_string(),
                request_hash: "hash-2".to_string(),
                request_id: "request-2".to_string(),
                turn_id: "turn-2".to_string(),
                message_id: "message-2".to_string(),
                lease_id: "lease-2".to_string(),
                now: 200,
                lease_expires_at: 210,
                record_expires_at: Some(300),
            })
            .await
            .unwrap(),
        IdempotencyReservation::Reserved(_)
    ));
}

#[tokio::test]
async fn session_clear_rejects_an_active_turn_without_changing_canonical_facts() {
    let original = session_record();
    let store = store_with_record(original.clone()).await;
    let admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();

    let error = store
        .clear_session_history("session-1", 200)
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
    assert_eq!(
        store.get_session_record("session-1").await.unwrap(),
        Some(original)
    );
    assert_eq!(
        store
            .list_agent_events_after("session-1", None, 10)
            .await
            .unwrap(),
        admission.events
    );
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        admission.active_turn
    );
}

#[tokio::test]
async fn missing_session_rolls_back_the_terminal_fact_and_nonterminal_deletion() {
    let store = store().await;
    let _admission = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    let terminal = event(
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );

    let missing = "missing-session".to_string();
    let mut fact_event = terminal;
    fact_event.identity.session_id = missing.clone();
    let cursor = agent_protocol::AgentSessionEventCursor {
        session_id: missing.clone(),
        generation: 0,
        offset: 1,
    };
    let error = store
        .project_worker_events(WorkerProjectionParams {
            execution_node_id: "embedded".into(),
            session_id: missing,
            after_cursor: None,
            page: agent_protocol::AgentSessionEventsResult {
                next_cursor: cursor.clone(),
                has_more: false,
                active_turn: None,
                events: vec![agent_protocol::AgentCommittedEvent {
                    cursor,
                    event: fact_event,
                    settlement: Some(agent_protocol::AgentExecutionSettlement::Finished {
                        metrics: AgentRunMetrics::default(),
                        memory_usage: vec![],
                    }),
                    execution_purpose: agent_protocol::AgentExecutionPurpose::Turn,
                    history_supplement: None,
                    automation_input: None,
                    command: None,
                    execution_model: None,
                }],
            },
        })
        .await
        .unwrap_err();
    let events = store
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap();

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert_eq!(events.len(), 2);
    assert_eq!(
        store
            .get_agent_active_turn("session-1")
            .await
            .unwrap()
            .map(|active| active.identity),
        Some(AgentTurnRef::new("session-1", "turn-1"))
    );
}

#[tokio::test]
async fn config_admission_checks_nonterminal_turns_only_on_the_target_node() {
    let mut session = session_record();
    session.execution_node_id = "computer-a".into();
    let store = store_with_record(session).await;
    let _admitted = store
        .project_test_events(initial_admission(admission()))
        .await
        .unwrap();
    assert!(store
        .has_nonterminal_turns(Some("computer-a"))
        .await
        .unwrap());
    assert!(!store
        .has_nonterminal_turns(Some("computer-b"))
        .await
        .unwrap());
    assert!(store.has_nonterminal_turns(None).await.unwrap());
    store
        .project_test_events(TestWorkerEvents {
            metrics: Some(AgentRunMetrics::default()),
            ..TestWorkerEvents::new(vec![event(
                3,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            )])
        })
        .await
        .map(|commit| {
            commit
                .events
                .into_iter()
                .last()
                .expect("terminal Worker projection")
        })
        .unwrap();
    assert!(!store
        .has_nonterminal_turns(Some("computer-a"))
        .await
        .unwrap());
}

#[path = "history_supplement_tests.rs"]
mod history_supplement_tests;
