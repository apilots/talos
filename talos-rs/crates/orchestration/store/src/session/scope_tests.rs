use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::migration::{run_migrations, test_migrations_dir};
use crate::project::{ProjectStore, StorageProjectStore};
use serde_json::Value;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole, AgentMessageStarted,
    AgentMessageStatus, AgentStopReason, AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
};
use talos_storage::{Row, SqliteStorage, Storage};
use talos_types::talos::{SessionSourceProvider, SessionSourceType, TokenUsage};
use talos_types::{ModelRef, ModelSelection, SessionConf, DEFAULT_SESSION_TITLE};

fn record(
    id: &str,
    user_id: &str,
    project_id: Option<String>,
    current_directory: Option<PathBuf>,
) -> SessionRecord {
    SessionRecord {
        id: id.to_string(),
        agent_id: "agent-a".to_string(),
        tree_identity: SessionTreeIdentity::Primary,
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: user_id.to_string(),
        source_type: SessionSourceType::Im,
        source_provider: SessionSourceProvider::Telegram,
        channel_id: Some("group-1".to_string()),
        thread_id: Some("topic-1".to_string()),
        environment: None,
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: talos_types::SessionUsage::default(),
        conf: SessionConf::default(),
        workspace_id: "workspace-alpha".to_string(),
        project_id,
        current_directory,
    }
}

async fn store() -> DatabaseSessionStore {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    DatabaseSessionStore::new(storage).await.unwrap()
}

async fn commit_completed_turn(
    store: &DatabaseSessionStore,
    session_id: &str,
    turn_id: &str,
) -> i64 {
    let identity = AgentTurnRef::new(session_id, turn_id);
    let started_message = AgentMessage {
        message_id: format!("message-{turn_id}"),
        turn_id: turn_id.to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::InProgress,
        blocks: Vec::new(),
        created_at: 10,
        completed_at: None,
    };
    let mut completed_message = started_message.clone();
    completed_message.status = AgentMessageStatus::Completed;
    completed_message.completed_at = Some(11);
    let admission = store
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: vec![
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 1,
                    occurred_at: 10,
                    payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
                },
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 2,
                    occurred_at: 10,
                    payload: AgentEventPayload::MessageStarted(AgentMessageStarted {
                        message: started_message,
                    }),
                },
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 3,
                    occurred_at: 11,
                    payload: AgentEventPayload::MessageCompleted(completed_message),
                },
            ],
            command: None,
        })
        .await
        .expect("canonical Turn admission should commit");
    store
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: AgentEvent {
                identity,
                sequence: 4,
                occurred_at: 12,
                payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: AgentStopReason::EndTurn,
                }),
            },
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("canonical Turn settlement should commit")
        .event_seq
}

#[test]
fn test_normalized_optional_string_treats_blank_as_none() {
    assert_eq!(DatabaseSessionStore::normalized_optional_string(None), None);
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some(String::new())),
        None
    );
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some("   ".to_string())),
        None
    );
    assert_eq!(
        DatabaseSessionStore::normalized_optional_string(Some(" project-1 ".to_string())),
        Some("project-1".to_string())
    );
}

#[tokio::test]
async fn test_insert_and_get_session_record_roundtrip() {
    let store = store().await;
    let mut record = record(
        "session-1",
        "user-1",
        Some("project-1".to_string()),
        Some(PathBuf::from("/repo")),
    );
    record.conf = SessionConf {
        model_selection: Some(ModelSelection {
            model_ref: ModelRef {
                provider: "deepseek".to_string(),
                id: "deepseek-v4-flash".to_string(),
            },
            reasoning_effort: None,
        }),
        ..SessionConf::default()
    };

    store.insert_session_record(&record).await.unwrap();
    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded, record);
}

#[tokio::test]
async fn subagent_identity_roundtrips_and_rejects_duplicate_tree_path() {
    let store = store().await;
    let parent = AgentTurnRef {
        session_id: "root-session".to_string(),
        turn_id: "root-turn".to_string(),
    };
    let mut child = record("child-1", "user-1", None, None);
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: parent.clone(),
        tree_root_session_id: "root-session".to_string(),
        path: "/root/reviewer".parse().unwrap(),
    };
    store.insert_session_record(&child).await.unwrap();

    let loaded = store.get_session_record("child-1").await.unwrap().unwrap();
    assert_eq!(loaded, child);

    let mut duplicate = record("child-2", "user-1", None, None);
    duplicate.tree_identity = SessionTreeIdentity::Subagent {
        parent,
        tree_root_session_id: "root-session".to_string(),
        path: "/root/reviewer".parse().unwrap(),
    };
    assert!(store.insert_session_record(&duplicate).await.is_err());
}

#[tokio::test]
async fn active_subagent_turn_check_uses_one_tree_query() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let mut child = record("child-session", "user-1", None, None);
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: AgentTurnRef {
            session_id: "root-session".to_string(),
            turn_id: "root-turn".to_string(),
        },
        tree_root_session_id: "root-session".to_string(),
        path: "/root/reviewer".parse().unwrap(),
    };
    store.insert_session_record(&child).await.unwrap();
    storage
        .execute(
            "INSERT INTO agent_active_turns (session_id, turn_id, state, last_event_seq, updated_at) VALUES (?, 'turn-1', 'running', 1, 1)",
            &[&child.id],
        )
        .await
        .unwrap();

    assert!(store
        .has_active_subagent_turn("root-session")
        .await
        .unwrap());
    assert!(!store.has_active_subagent_turn("other-root").await.unwrap());
}

#[tokio::test]
async fn delete_session_tree_removes_descendants_and_runtime_rows() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let root = record("root-session", "user-1", None, None);
    let mut child = record("child-session", "user-1", None, None);
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: AgentTurnRef {
            session_id: root.id.clone(),
            turn_id: "root-turn".to_string(),
        },
        tree_root_session_id: root.id.clone(),
        path: "/root/reviewer".parse().unwrap(),
    };
    store.insert_session_record(&root).await.unwrap();
    store.insert_session_record(&child).await.unwrap();
    for session_id in [&root.id, &child.id] {
        storage
            .execute(
                "INSERT INTO agent_events (session_id, turn_id, sequence, occurred_at, persisted_at, payload_json) VALUES (?, 'turn-1', 1, 1, 1, '{}')",
                &[session_id],
            )
            .await
            .unwrap();
        storage
            .execute(
                "INSERT INTO agent_active_turns (session_id, turn_id, state, last_event_seq, updated_at) VALUES (?, 'turn-1', 'running', 1, 1)",
                &[session_id],
            )
            .await
            .unwrap();
        storage
            .execute(
                "INSERT INTO agent_command_idempotency (session_id, command, idempotency_key, request_hash, request_id, turn_id, message_id, status, lease_id, lease_expires_at, created_at, updated_at) VALUES (?, 'turn_submission', 'key-1', 'hash-1', 'request-1', 'turn-1', 'message-1', 'reserved', 'lease-1', 2, 1, 1)",
                &[session_id],
            )
            .await
            .unwrap();
    }

    assert!(store.delete_session_tree(&root.id).await.unwrap());

    let counts = storage
        .query(
            "SELECT (SELECT COUNT(*) FROM sessions) AS sessions, (SELECT COUNT(*) FROM agent_events) AS events, (SELECT COUNT(*) FROM agent_active_turns) AS active_turns, (SELECT COUNT(*) FROM agent_command_idempotency) AS idempotency_keys",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(
        (
            counts[0].get_i64("sessions"),
            counts[0].get_i64("events"),
            counts[0].get_i64("active_turns"),
            counts[0].get_i64("idempotency_keys"),
        ),
        (Some(0), Some(0), Some(0), Some(0))
    );
}

#[tokio::test]
async fn incompatible_typed_conf_is_rejected_on_read() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let record = record("session-1", "user-1", None, None);
    store.insert_session_record(&record).await.unwrap();
    storage
        .update(
            "sessions",
            &Row::new(
                vec!["conf_json".to_string()],
                vec![Value::String("\"invalid\"".to_string())],
            ),
            "session_key = ?",
            &["session-1"],
        )
        .await
        .unwrap();

    let error = store
        .get_session_record("session-1")
        .await
        .expect_err("invalid session conf should fail");

    assert!(error
        .to_string()
        .contains("parse session 'session-1' conf_json"));
}

#[tokio::test]
async fn session_patch_updates_only_settings_owned_columns() {
    let store = store().await;
    let record = record("session-1", "user-1", Some("project-1".to_string()), None);
    store.insert_session_record(&record).await.unwrap();
    let terminal_event_seq = commit_completed_turn(&store, "session-1", "turn-1").await;
    let terminal_event_seq =
        u64::try_from(terminal_event_seq).expect("terminal event sequence should be positive");
    store
        .advance_memory_processed_until_event_seq("session-1", 0, terminal_event_seq)
        .await
        .unwrap();
    let measured_context = talos_types::ContextUsage::new(7, 100, 80, 12).unwrap();
    store
        .update_context_usage("session-1", &measured_context)
        .await
        .unwrap();
    let conf = SessionConf {
        model_selection: Some(ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.4".to_string(),
        })),
        ..SessionConf::default()
    };
    let updated = store
        .patch_session(PatchSessionParams {
            session_id: "session-1".to_string(),
            conf: conf.clone(),
            current_directory: Some(PathBuf::from("/tmp/updated")),
            reset_context_usage: true,
            last_activity: 42,
        })
        .await
        .unwrap()
        .unwrap();

    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded, updated);
    assert_eq!(loaded.title, DEFAULT_SESSION_TITLE);
    assert_eq!(loaded.message_count, 0);
    assert_eq!(loaded.memory_processed_until_event_seq, terminal_event_seq);
    assert_eq!(loaded.latest_turn_seq, 0);
    assert_eq!(loaded.usage.context, None);
    assert_eq!(loaded.user_id, "user-1");
    assert_eq!(loaded.conf, conf);
    assert_eq!(loaded.agent_id, "agent-a");
    assert_eq!(loaded.workspace_id, "workspace-alpha");
    assert_eq!(loaded.project_id.as_deref(), Some("project-1"));
    assert_eq!(
        loaded.current_directory,
        Some(PathBuf::from("/tmp/updated"))
    );
}

#[tokio::test]
async fn context_usage_update_preserves_the_rest_of_the_session_record() {
    let store = store().await;
    let mut record = record("session-1", "user-1", None, None);
    record.title = "Concurrent title".to_string();
    record.conf.model_selection = Some(ModelSelection::without_reasoning(ModelRef {
        provider: "openai".to_string(),
        id: "gpt-5.4".to_string(),
    }));
    record.usage.tokens = TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        ..TokenUsage::default()
    };
    store.insert_session_record(&record).await.unwrap();

    let context = talos_types::ContextUsage::new(17, 100, 80, 12).unwrap();
    assert!(store
        .update_context_usage("session-1", &context)
        .await
        .unwrap());

    let loaded = store
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.title, "Concurrent title");
    assert_eq!(loaded.conf, record.conf);
    assert_eq!(loaded.usage.tokens, record.usage.tokens);
    assert_eq!(loaded.usage.context, Some(context));
}

#[tokio::test]
async fn memory_cursor_advance_is_idempotent() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None))
        .await
        .unwrap();
    let terminal_event_seq = commit_completed_turn(&store, "session-1", "turn-1").await;
    let terminal_event_seq =
        u64::try_from(terminal_event_seq).expect("terminal event sequence should be positive");

    store
        .advance_memory_processed_until_event_seq("session-1", 0, terminal_event_seq)
        .await
        .unwrap();
    store
        .advance_memory_processed_until_event_seq("session-1", 0, terminal_event_seq)
        .await
        .unwrap();

    assert_eq!(
        store
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap()
            .memory_processed_until_event_seq,
        terminal_event_seq
    );
}

#[tokio::test]
async fn memory_cursor_rejects_a_stale_expected_value() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None))
        .await
        .unwrap();
    let terminal_event_seq = commit_completed_turn(&store, "session-1", "turn-1").await;
    let terminal_event_seq =
        u64::try_from(terminal_event_seq).expect("terminal event sequence should be positive");

    let error = store
        .advance_memory_processed_until_event_seq("session-1", 1, terminal_event_seq)
        .await
        .expect_err("stale cursor should fail");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
}

#[tokio::test]
async fn test_delete_session_record_removes_session() {
    let store = store().await;
    let record = record("session-1", "user-1", None, None);
    store.insert_session_record(&record).await.unwrap();

    assert!(store.delete_session_record("session-1").await.unwrap());
    assert!(store
        .get_session_record("session-1")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_list_session_records_filters_by_project_id() {
    let store = store().await;
    store
        .insert_session_record(&record(
            "session-1",
            "user-1",
            Some("project-1".to_string()),
            None,
        ))
        .await
        .unwrap();
    store
        .insert_session_record(&record(
            "session-2",
            "user-1",
            Some("project-2".to_string()),
            None,
        ))
        .await
        .unwrap();

    let records = store
        .list_session_records(&SessionQuery {
            project_id: Some("project-1".to_string()),
            ..SessionQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "session-1");
}

#[tokio::test]
async fn test_list_session_records_respects_limit_and_offset() {
    let store = store().await;
    store
        .insert_session_record(&record("session-1", "user-1", None, None))
        .await
        .unwrap();
    store
        .insert_session_record(&record("session-2", "user-1", None, None))
        .await
        .unwrap();
    store
        .insert_session_record(&record("session-3", "user-1", None, None))
        .await
        .unwrap();

    let records = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            offset: Some(1),
            ..SessionQuery::default()
        })
        .await
        .unwrap();

    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn memory_pending_session_pages_use_a_stable_cursor() {
    let store = store().await;
    let mut processed = record("session-1", "user-1", None, None);
    processed.last_activity = 3;
    let mut first_pending = record("session-2", "user-1", None, None);
    first_pending.last_activity = 2;
    let mut second_pending = record("session-3", "user-1", None, None);
    second_pending.last_activity = 1;
    for session in [&processed, &first_pending, &second_pending] {
        store
            .insert_session_record(session)
            .await
            .expect("insert session");
    }
    let processed_event_seq = commit_completed_turn(&store, "session-1", "turn-1").await;
    store
        .advance_memory_processed_until_event_seq(
            "session-1",
            0,
            u64::try_from(processed_event_seq).expect("event sequence should be positive"),
        )
        .await
        .expect("processed cursor should advance");
    commit_completed_turn(&store, "session-2", "turn-2").await;
    commit_completed_turn(&store, "session-3", "turn-3").await;

    let first_page = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            memory_pending_only: true,
            ..SessionQuery::default()
        })
        .await
        .expect("first pending page");
    let second_page = store
        .list_session_records(&SessionQuery {
            limit: Some(1),
            memory_pending_only: true,
            after: Some(SessionPageCursor {
                last_activity: first_page[0].last_activity,
                session_id: first_page[0].id.clone(),
            }),
            ..SessionQuery::default()
        })
        .await
        .expect("second pending page");

    assert_eq!(first_page[0].id, "session-2");
    assert_eq!(second_page[0].id, "session-3");
}

#[tokio::test]
async fn memory_pending_uses_canonical_terminal_events_after_message_count_reset() {
    let store = store().await;
    let mut cleared = record("session-1", "user-1", None, None);
    cleared.message_count = 0;
    cleared.latest_turn_seq = 101;
    store
        .insert_session_record(&cleared)
        .await
        .expect("insert cleared session");
    commit_completed_turn(&store, "session-1", "turn-101").await;

    let records = store
        .list_session_records(&SessionQuery {
            memory_pending_only: true,
            ..SessionQuery::default()
        })
        .await
        .expect("list pending sessions");

    assert_eq!(
        records
            .into_iter()
            .map(|record| (record.id, record.latest_turn_seq))
            .collect::<Vec<_>>(),
        vec![("session-1".to_string(), 101)]
    );
}

#[tokio::test]
async fn test_list_projects_returns_session_count() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_migrations(storage.pool(), &test_migrations_dir())
        .await
        .unwrap();
    let store = DatabaseSessionStore::new(storage.clone()).await.unwrap();
    let project_store = StorageProjectStore::new(storage).await.unwrap();
    let project = project_store
        .create_project("repo", &PathBuf::from("/repo"))
        .await
        .unwrap();
    store
        .insert_session_record(&record(
            "session-1",
            "user-1",
            Some(project.project_id.clone()),
            None,
        ))
        .await
        .unwrap();

    let projects = project_store.list_projects().await.unwrap();

    assert_eq!(projects[0].session_count, 1);
}
