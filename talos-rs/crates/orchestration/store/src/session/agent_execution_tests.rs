use std::sync::Arc;
use std::{ops::Deref, path::PathBuf};

use talos_agent_api::{
    AgentContinuationRef, AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole,
    AgentMessageStatus, AgentPendingInput, AgentRunMetrics, AgentStopReason, AgentTurnCompleted,
    AgentTurnRef, AgentTurnStarted,
};
use talos_storage::SqliteStorage;
use talos_types::talos::{SessionSourceProvider, SessionSourceType};
use talos_types::{ContextUsage, SessionConf, SessionUsage, TokenUsage};

use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use crate::session::{
    AccumulateSessionUsageParams, DatabaseSessionStore, SessionRecord, SessionStore,
    SessionTreeIdentity,
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
    TestStore {
        store: DatabaseSessionStore::new(storage).await.unwrap(),
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

fn initial_admission(events: Vec<AgentEvent>) -> CommitInitialAdmissionParams {
    CommitInitialAdmissionParams {
        events,
        command: None,
    }
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
    SessionRecord {
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
        current_directory: None,
    }
}

#[tokio::test]
async fn failed_initial_admission_rolls_back_its_event_batch() {
    let store = store().await;
    store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();

    let second_identity = AgentTurnRef::new("session-1", "turn-2");
    assert!(store
        .commit_initial_admission(initial_admission(admission_for(
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
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();

    assert_eq!(commit.events.len(), 2);
    assert_eq!(
        commit.active_turn.initial_message_id.as_deref(),
        Some("message-1")
    );
    assert_eq!(commit.active_turn.last_event_seq, 2);
    assert_eq!(
        store.get_agent_active_turn("session-1").await.unwrap(),
        Some(commit.active_turn)
    );
}

#[tokio::test]
async fn maintenance_admission_does_not_fabricate_an_initial_message() {
    let store = store().await;

    let commit = store
        .commit_maintenance_admission(event(
            1,
            AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        ))
        .await
        .unwrap();

    assert_eq!(commit.active_turn.initial_message_id, None);
}

#[tokio::test]
async fn progress_commit_requires_the_exact_durable_tail() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let progress = event(
        3,
        AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
    );

    assert!(store
        .commit_progress_event(progress.clone(), 1)
        .await
        .is_err());
    let commit = store
        .commit_progress_event(progress, admission.active_turn.last_event_seq)
        .await
        .unwrap();

    assert_eq!(commit.events[0].sequence, 3);
    assert_eq!(commit.active_turn.last_event_seq, 3);
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
async fn running_input_admission_advances_the_same_active_turn() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();

    let commit = store
        .commit_running_input_admission(CommitRunningInputAdmissionParams {
            events: user_admission(3, "message-2"),
            expected_event_seq: admission.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap();

    assert_eq!(
        (
            commit.events[0].sequence,
            commit.active_turn.initial_message_id.as_deref(),
            commit.active_turn.state,
        ),
        (3, Some("message-1"), ActiveTurnState::Running)
    );
}

#[tokio::test]
async fn continuation_claim_is_idempotent_for_the_same_claim() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let waiting = store
        .commit_waiting_settlement(
            waiting(3, "action-1"),
            AgentContinuationRef {
                opaque_id: "continuation-1".to_string(),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let events = user_admission(4, "message-2");

    let first = store
        .claim_continuation(ClaimContinuationParams {
            events: events.clone(),
            action_id: "action-1".to_string(),
            claim_id: "claim-1".to_string(),
            expected_event_seq: waiting.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap();
    let mut reconstructed_events = events;
    reconstructed_events[0].occurred_at = 20;
    let retry = store
        .claim_continuation(ClaimContinuationParams {
            events: reconstructed_events,
            action_id: "action-1".to_string(),
            claim_id: "claim-1".to_string(),
            expected_event_seq: waiting.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap();

    assert_eq!(retry, first);
}

#[tokio::test]
async fn later_waiting_replaces_the_claim_baseline_after_progress() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let first_waiting = store
        .commit_waiting_settlement(
            waiting(3, "action-1"),
            AgentContinuationRef {
                opaque_id: "continuation-1".to_string(),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let claim = store
        .claim_continuation(ClaimContinuationParams {
            events: user_admission(4, "message-2"),
            action_id: "action-1".to_string(),
            claim_id: "claim-1".to_string(),
            expected_event_seq: first_waiting.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap();
    let progress = store
        .commit_progress_event(
            event(
                5,
                AgentEventPayload::MetricsUpdated(talos_agent_api::AgentRunMetrics::default()),
            ),
            claim.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    let second_waiting = store
        .commit_waiting_settlement(
            waiting(6, "action-2"),
            AgentContinuationRef {
                opaque_id: "continuation-2".to_string(),
            },
            progress.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let binding = store
        .get_agent_continuation_binding(&AgentTurnRef::new("session-1", "turn-1"))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        (
            second_waiting.active_turn.state,
            binding.action_id.as_str(),
            binding.provider_ref.opaque_id.as_str(),
            binding.expected_event_seq,
            binding.status,
        ),
        (
            ActiveTurnState::WaitingForInput,
            "action-2",
            "continuation-2",
            second_waiting.active_turn.last_event_seq,
            AgentContinuationBindingStatus::Waiting,
        )
    );
}

#[tokio::test]
async fn conflicting_continuation_claim_does_not_append_input_twice() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let waiting = store
        .commit_waiting_settlement(
            waiting(3, "action-1"),
            AgentContinuationRef {
                opaque_id: "continuation-1".to_string(),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let events = user_admission(4, "message-2");
    store
        .claim_continuation(ClaimContinuationParams {
            events: events.clone(),
            action_id: "action-1".to_string(),
            claim_id: "claim-1".to_string(),
            expected_event_seq: waiting.active_turn.last_event_seq,
            command: None,
        })
        .await
        .unwrap();

    let conflict = store
        .claim_continuation(ClaimContinuationParams {
            events,
            action_id: "action-1".to_string(),
            claim_id: "claim-2".to_string(),
            expected_event_seq: waiting.active_turn.last_event_seq,
            command: None,
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
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let waiting = store
        .commit_waiting_settlement(
            waiting(3, "action-1"),
            AgentContinuationRef {
                opaque_id: "continuation-1".to_string(),
            },
            admission.active_turn.last_event_seq,
        )
        .await
        .unwrap();

    let terminal = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                4,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            ),
            expected_event_seq: waiting.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    let identity = AgentTurnRef::new("session-1", "turn-1");

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
            store
                .get_agent_continuation_binding(&identity)
                .await
                .unwrap(),
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
            None,
        )
    );
}

#[tokio::test]
async fn terminal_settlement_and_compaction_usage_update_disjoint_session_state() {
    let store = store().await;
    store
        .insert_session_record(&session_record())
        .await
        .unwrap();
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let metrics = AgentRunMetrics {
        usage: TokenUsage {
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        context: ContextUsage::new(9, 32, 24, 10),
        ..AgentRunMetrics::default()
    };

    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                3,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            ),
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: Some(metrics.clone()),
        })
        .await
        .unwrap();
    let compaction_usage = TokenUsage {
        input_tokens: 1,
        output_tokens: 1,
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
async fn session_clear_atomically_removes_canonical_history_and_resets_summary() {
    let store = store().await;
    let original = session_record();
    store.insert_session_record(&original).await.unwrap();
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
    let admission = store
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: admission(),
            command: Some(lease),
        })
        .await
        .unwrap();
    let metrics = AgentRunMetrics {
        usage: TokenUsage {
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        context: ContextUsage::new(9, 32, 24, 10),
        ..AgentRunMetrics::default()
    };
    let terminal = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                3,
                AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            ),
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: Some(metrics),
        })
        .await
        .unwrap();
    store
        .advance_memory_processed_until_event_seq(
            "session-1",
            0,
            terminal.event_seq.try_into().unwrap(),
        )
        .await
        .unwrap();

    assert!(store.clear_session_history("session-1", 200).await.unwrap());

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
    let store = store().await;
    let original = session_record();
    store.insert_session_record(&original).await.unwrap();
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
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
        Some(admission.active_turn)
    );
}

#[tokio::test]
async fn missing_session_rolls_back_the_terminal_fact_and_nonterminal_deletion() {
    let store = store().await;
    let admission = store
        .commit_initial_admission(initial_admission(admission()))
        .await
        .unwrap();
    let terminal = event(
        3,
        AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    );

    let error = store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: terminal,
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: Some(AgentRunMetrics::default()),
        })
        .await
        .unwrap_err();
    let events = store
        .list_agent_turn_events(&AgentTurnRef::new("session-1", "turn-1"), 10)
        .await
        .unwrap();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
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
