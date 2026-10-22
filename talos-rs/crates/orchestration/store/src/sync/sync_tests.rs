use super::*;
use crate::node::{AgentNodeStore, DatabaseAgentNodeStore};
use crate::project::{ProjectStore, StorageProjectStore};
use crate::session::*;
use talos_agent_api::*;
use talos_auth::{RequestIdentity, UserRole};
use talos_storage::{SqliteStorage, Storage};
use talos_sync::{ComputerAlias, SyncBindParams, SyncOptions, SyncScope};
use talos_types::*;

#[path = "read_snapshot_tests.rs"]
mod read_snapshot_tests;

const COMPUTER: &str = "d4383755-33d1-43fd-a6ef-cf14dc2a823d";
struct Peer {
    storage: Arc<SqliteStorage>,
    sync: Arc<TalosSync>,
    adapter: Arc<StoreSyncAdapter>,
    sessions: DatabaseSessionStore,
    _home: tempfile::TempDir,
}
fn projection(record: &SessionRecord) -> TalosResult<SessionInfo> {
    Ok(SessionInfo {
        node_id: record.execution_node_id.clone(),
        id: record.id.clone(),
        title: record.title.clone(),
        user_id: record.user_id.clone(),
        source_type: record.source_type.clone(),
        source_provider: record.source_provider.clone(),
        model_selection: ModelSelection::without_reasoning(ModelRef {
            provider: "worker-provider".into(),
            id: "worker-model".into(),
        }),
        configured_default_mode: UserPermissionMode::AskForApproval,
        effective_mode: record
            .conf
            .user_permission_mode
            .unwrap_or(UserPermissionMode::AskForApproval),
        created_at: record.created_at,
        last_activity: record.last_activity,
        message_count: record.message_count,
        usage: record.usage.clone(),
        project_id: record.project_id.clone(),
        current_directory: record.current_directory.clone(),
    })
}
async fn peer(local: bool) -> Peer {
    let home = tempfile::tempdir().unwrap();
    let storage = Arc::new(
        SqliteStorage::new(home.path().join("facts.db").to_str().unwrap())
            .await
            .unwrap(),
    );
    crate::migration::run_migrations(storage.pool(), &crate::migration::test_migrations_dir())
        .await
        .unwrap();
    let sync = Arc::new(
        TalosSync::open(
            storage.clone(),
            SyncOptions {
                hub_endpoint: "wss://sync.example.test/v1/sync".into(),
            },
        )
        .await
        .unwrap(),
    );
    let actor = RequestIdentity {
        user_id: "account-1".into(),
        username: None,
        role: UserRole::User,
        auth_disabled: false,
        access_token: None,
    };
    let binding = sync
        .bind_account(
            &actor,
            SyncBindParams {
                scope: SyncScope {
                    hub_id: "hub-1".into(),
                    account_id: actor.user_id.clone(),
                },
                source_user_id: actor.user_id.clone(),
                computers: if local {
                    vec![ComputerAlias {
                        source_node_id: "embedded".into(),
                        computer_id: COMPUTER.into(),
                    }]
                } else {
                    vec![]
                },
            },
        )
        .await
        .unwrap();
    let adapter = Arc::new(StoreSyncAdapter::new(
        storage.clone(),
        sync.clone(),
        binding,
        Arc::new(projection),
    ));
    let sessions = DatabaseSessionStore::new(storage.clone())
        .await
        .unwrap()
        .with_sync(adapter.clone());
    Peer {
        storage,
        sync,
        adapter,
        sessions,
        _home: home,
    }
}
async fn registered_computer(peer: &Peer) {
    DatabaseAgentNodeStore::new(peer.storage.pool().clone())
        .with_sync(peer.adapter.clone())
        .create(crate::node::CreateNodeParams {
            node_id: COMPUTER.into(),
            owner_user_id: Some("account-1".into()),
            display_name: "Owned Computer".into(),
            mode: NodeMode::Remote,
            endpoint: None,
            credential_hash: None,
            credential_expires_at: None,
            now_at: 1,
        })
        .await
        .unwrap();
}
fn record(id: &str) -> SessionRecord {
    SessionRecord {
        id: id.into(),
        agent_id: "default".into(),
        execution_node_id: "embedded".into(),
        tree_identity: SessionTreeIdentity::Primary,
        workspace_id: "workspace-1".into(),
        title: "Original".into(),
        user_id: "account-1".into(),
        source_type: SessionSourceType::Desktop,
        source_provider: SessionSourceProvider::Sdk,
        channel_id: Some("source-channel".into()),
        thread_id: Some("source-thread".into()),
        environment: Some(TalosSessionEnvironment {
            directory: "/worker/home".into(),
            connection_mode: TalosConnectionMode::Embedded,
        }),
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: SessionUsage::default(),
        conf: SessionConf {
            model_selection: None,
            user_permission_mode: Some(UserPermissionMode::AskForApproval),
        },
        project_id: None,
        current_directory: Some("/worker/custom".into()),
    }
}
fn event(id: &str, turn: &str, sequence: u64, payload: AgentEventPayload) -> AgentEvent {
    AgentEvent {
        identity: AgentTurnRef::new(id, turn),
        sequence,
        occurred_at: 10,
        payload,
    }
}
async fn run(peer: &Peer, session: &str, turn: &str) {
    let identity = AgentTurnRef::new(session, turn);
    let started = event(
        session,
        turn,
        1,
        AgentEventPayload::TurnStarted(AgentTurnStarted {}),
    );
    let message = event(
        session,
        turn,
        2,
        AgentEventPayload::MessageCompleted(AgentMessage {
            message_id: format!("message-{turn}"),
            turn_id: turn.into(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            blocks: vec![],
            created_at: 10,
            completed_at: Some(10),
        }),
    );
    let commit = peer
        .sessions
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: vec![started, message],
            command: None,
        })
        .await
        .unwrap();
    peer.sessions.begin_cancellation(&identity).await.unwrap();
    peer.sessions
        .commit_terminal_settlement(CommitTerminalSettlementParams {
            event: event(
                session,
                turn,
                3,
                AgentEventPayload::TurnFailed(AgentTurnFailed {
                    error: talos_error::ApiError::with_message(
                        talos_error::ErrorCode::ProviderUnavailable,
                        "Provider became unavailable",
                    ),
                    retryable: true,
                }),
            ),
            expected_event_seq: commit.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .unwrap();
    peer.sessions
        .append_timeline_notice(TimelineSystemNotice {
            identity,
            occurred_at: 11,
            notice: AgentSystemNotice {
                level: TalosTimelineNoticeLevel::Info,
                content: SystemNoticeContent::Text("Original completion notice".into()),
            },
        })
        .await
        .unwrap();
}
async fn exchange(source: &Peer, destination: &Peer, resource: SyncResourceKey) {
    while let Some(batch) = source
        .sync
        .export_pending(&source.adapter.binding, resource.clone(), 128)
        .await
        .unwrap()
    {
        let mut tx = destination.storage.begin_transaction().await.unwrap();
        let (version, changes) = destination
            .sync
            .import_document(
                &mut *tx,
                &destination.adapter.binding,
                &batch.resource,
                std::slice::from_ref(&batch.update),
                &batch.version,
                None,
            )
            .await
            .unwrap();
        destination
            .adapter
            .apply(
                &mut *tx,
                &SyncImportBatch {
                    scope: destination.adapter.binding.scope.clone(),
                    resource: batch.resource.clone(),
                    version,
                    changes,
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        source.sync.acknowledge(&batch).await.unwrap();
    }
}
#[tokio::test]
async fn original_stores_capture_full_facts_and_materialize_without_execution_or_recapture() {
    let a = peer(true).await;
    let b = peer(false).await;
    registered_computer(&a).await;
    let projects = StorageProjectStore::new(a.storage.clone())
        .await
        .unwrap()
        .with_sync(a.adapter.clone());
    let project = projects
        .create_project_on_node(
            "Empty project",
            std::path::Path::new("/worker/project"),
            "account-1",
            "embedded",
        )
        .await
        .unwrap();
    exchange(
        &a,
        &b,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    exchange(
        &a,
        &b,
        SyncResourceKey::Project {
            project_id: project.project_id.clone(),
        },
    )
    .await;
    let remote = DatabaseAgentNodeStore::new(b.storage.pool().clone())
        .get(COMPUTER)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(remote.mode, NodeMode::Remote);
    assert_eq!(remote.lease_token_hash, None);
    let imported_project = StorageProjectStore::new(b.storage.clone())
        .await
        .unwrap()
        .get_project(&project.project_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(imported_project.execution_node_id, COMPUTER);
    assert_eq!(imported_project.session_count, 0);
    a.sessions
        .insert_session_record(&record("session-1"))
        .await
        .unwrap();
    run(&a, "session-1", "z-first").await;
    run(&a, "session-1", "a-second").await;
    let resource = SyncResourceKey::Session {
        session_id: "session-1".into(),
    };
    exchange(&a, &b, resource.clone()).await;
    let imported = b
        .sessions
        .get_session_record("session-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(imported.conf, record("session-1").conf);
    assert_eq!(
        imported.current_directory,
        record("session-1").current_directory
    );
    assert_eq!(imported.execution_node_id, COMPUTER);
    assert_eq!(imported.environment, record("session-1").environment);
    let source = a
        .sessions
        .list_timeline_events_after("session-1", 0, 100)
        .await
        .unwrap();
    let target = b
        .sessions
        .list_timeline_events_after("session-1", 0, 100)
        .await
        .unwrap();
    assert_eq!(
        source.iter().map(|e| &e.event).collect::<Vec<_>>(),
        target.iter().map(|e| &e.event).collect::<Vec<_>>()
    );
    assert_eq!(
        b.sessions.get_agent_active_turn("session-1").await.unwrap(),
        None
    );
    assert!(b
        .sync
        .pending_resources(&b.adapter.binding, 128)
        .await
        .unwrap()
        .is_empty());
    a.sessions
        .clear_session_history("session-1", 100)
        .await
        .unwrap();
    exchange(&a, &b, resource.clone()).await;
    assert!(b
        .sessions
        .list_timeline_events_after("session-1", 0, 100)
        .await
        .unwrap()
        .is_empty());
    run(&a, "session-1", "new-generation").await;
    exchange(&a, &b, resource.clone()).await;
    assert_eq!(
        b.sessions
            .list_timeline_events_after("session-1", 0, 100)
            .await
            .unwrap()
            .len(),
        4
    );
    a.sessions.delete_session_tree("session-1").await.unwrap();
    exchange(&a, &b, resource).await;
    assert_eq!(
        b.sessions.get_session_record("session-1").await.unwrap(),
        None
    );
}
#[tokio::test]
async fn failed_store_admission_rolls_back_facts_and_capture_together() {
    let a = peer(true).await;
    a.sessions
        .insert_session_record(&record("session-1"))
        .await
        .unwrap();
    let before = a
        .storage
        .query("SELECT COUNT(*) AS count FROM sync_outbox", &[])
        .await
        .unwrap()[0]
        .get_i64("count");
    let error = a
        .sessions
        .commit_initial_admission(CommitInitialAdmissionParams {
            events: vec![event(
                "session-1",
                "turn-1",
                1,
                AgentEventPayload::TurnStarted(AgentTurnStarted {}),
            )],
            command: None,
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_api_error().code, talos_error::ErrorCode::Conflict);
    assert_eq!(
        a.storage
            .query("SELECT COUNT(*) AS count FROM sync_outbox", &[])
            .await
            .unwrap()[0]
            .get_i64("count"),
        before
    );
    assert!(a
        .sessions
        .list_timeline_events_after("session-1", 0, 100)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn file_source_and_child_identity_survive_import_and_source_deletion() {
    use crate::file::{
        DatabaseFileStore, FileLocation, FileReadyUpdate, FileRecord, FileStatus, FileStore,
    };
    let a = peer(true).await;
    let b = peer(false).await;
    registered_computer(&a).await;
    exchange(
        &a,
        &b,
        SyncResourceKey::Computer {
            computer_id: COMPUTER.into(),
        },
    )
    .await;
    a.sessions
        .insert_session_record(&record("z-parent"))
        .await
        .unwrap();
    run(&a, "z-parent", "parent-turn").await;
    let mut child = record("a-child");
    child.tree_identity = SessionTreeIdentity::Subagent {
        parent: AgentTurnRef::new("z-parent", "parent-turn"),
        tree_root_session_id: "z-parent".into(),
        path: "/root/helper".parse().unwrap(),
    };
    a.sessions.insert_session_record(&child).await.unwrap();
    let resource = SyncResourceKey::Session {
        session_id: child.id.clone(),
    };
    let batch = a
        .sync
        .export_pending(&a.adapter.binding, resource.clone(), 128)
        .await
        .unwrap()
        .unwrap();
    let mut tx = b.storage.begin_transaction().await.unwrap();
    let (version, changes) = b
        .sync
        .import_document(
            &mut *tx,
            &b.adapter.binding,
            &resource,
            std::slice::from_ref(&batch.update),
            &batch.version,
            None,
        )
        .await
        .unwrap();
    let dependencies = b
        .adapter
        .missing_dependencies(
            &mut *tx,
            &SyncImportBatch {
                scope: b.adapter.binding.scope.clone(),
                resource,
                version,
                changes,
            },
        )
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        dependencies,
        vec![SyncResourceKey::Session {
            session_id: "z-parent".into()
        }]
    );
    exchange(
        &a,
        &b,
        SyncResourceKey::Session {
            session_id: "z-parent".into(),
        },
    )
    .await;
    exchange(
        &a,
        &b,
        SyncResourceKey::Session {
            session_id: child.id.clone(),
        },
    )
    .await;
    assert_eq!(
        b.sessions
            .get_session_record(&child.id)
            .await
            .unwrap()
            .unwrap()
            .tree_identity,
        child.tree_identity
    );
    let files = DatabaseFileStore::new(a.storage.pool().clone()).with_sync(a.adapter.clone());
    files
        .create_file(FileRecord {
            file_id: "file-1".into(),
            owner_user_id: "account-1".into(),
            project_id: None,
            location: FileLocation::Local {
                storage_id: "local".into(),
                object_key: "files/file-1/body".into(),
            },
            original_filename: "original.txt".into(),
            mime_type: None,
            size_bytes: None,
            status: FileStatus::Uploading,
            tags: vec!["original".into()],
            metadata: Default::default(),
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    files
        .mark_ready(FileReadyUpdate {
            file_id: "file-1".into(),
            mime_type: "text/plain".into(),
            size_bytes: 3,
            updated_at: 2,
        })
        .await
        .unwrap();
    let resource = SyncResourceKey::File {
        file_id: "file-1".into(),
    };
    exchange(&a, &b, resource.clone()).await;
    let remote = DatabaseFileStore::new(b.storage.pool().clone())
        .get_file_for_owner(&crate::file::FileOwnerKey {
            file_id: "file-1".into(),
            owner_user_id: "account-1".into(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        remote.location,
        FileLocation::Peer {
            peer_id: a.sync.peer_id().into()
        }
    );
    assert_eq!(remote.tags, vec!["original"]);
    let key = crate::file::FileOwnerKey {
        file_id: "file-1".into(),
        owner_user_id: "account-1".into(),
    };
    files.mark_deleting_for_owner(&key, 3).await.unwrap();
    files.delete_file_record("file-1").await.unwrap();
    exchange(&a, &b, resource).await;
    assert!(DatabaseFileStore::new(b.storage.pool().clone())
        .get_file_for_owner(&key)
        .await
        .unwrap()
        .is_none());
    a.sessions.delete_session_tree("z-parent").await.unwrap();
    exchange(
        &a,
        &b,
        SyncResourceKey::Session {
            session_id: child.id.clone(),
        },
    )
    .await;
    assert!(b
        .sessions
        .get_session_record(&child.id)
        .await
        .unwrap()
        .is_none());
}

#[path = "projection_tests.rs"]
mod projection_tests;

#[path = "deletion_tests.rs"]
mod deletion_tests;
