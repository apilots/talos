//! Local resource scope uses the product owner's persisted Session binding.
use super::*;
use std::path::Path;
use talos_agent::{AttachmentReader, ResolvedAttachment, TurnFileScope};
use talos_storage::SqliteStorage;
use talos_store::session::{DatabaseSessionStore, SessionTreeIdentity};
use talos_types::talos::{SessionSourceProvider, SessionSourceType};
use talos_types::{SessionConf, SessionUsage};

#[tokio::test]
async fn local_resource_scope_is_bound_to_the_persisted_session() {
    let storage = Arc::new(
        SqliteStorage::new_with_max_connections(":memory:", 1)
            .await
            .unwrap(),
    );
    talos_store::migration::run_migrations(
        storage.pool(),
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();
    let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let mut record = session_record();
    record.execution_node_id = "embedded".into();
    sessions.insert_session_record(&record).await.unwrap();
    let data = NodeSessionDataRpc::local(sessions, Arc::new(TestAttachments));
    let context = agent_protocol::AgentSessionContext {
        agent_id: record.agent_id.clone(),
        session_id: record.id.clone(),
        workspace: talos_config::Config::default()
            .resolve_workspace("default", "user-1")
            .unwrap(),
    };
    data.authorize_local_session(&context).await.unwrap();
    let mut forged = context.clone();
    forged.workspace.user_id = "different-user".into();
    assert!(data
        .authorize_local_session(&forged)
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    let mut unrelated = context;
    unrelated.session_id = "unrelated-session".into();
    assert!(data.authorize_local_session(&unrelated).await.is_err());
}

pub(crate) struct TestAttachments;

impl AttachmentReader for TestAttachments {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>,
    > {
        Box::pin(async move {
            assert_eq!(scope.owner_user_id, "user-1");
            assert_eq!(scope.project_id.as_deref(), Some("project-1"));
            assert_eq!(file_ids, ["file-1"]);
            Ok(vec![ResolvedAttachment {
                file_id: "file-1".into(),
                filename: "input.txt".into(),
                media_type: "text/plain".into(),
                size_bytes: 7,
                bytes: bytes::Bytes::from_static(b"content"),
            }])
        })
    }
}

pub(crate) fn session_record() -> SessionRecord {
    SessionRecord {
        session_kind: talos_types::SessionKind::Standard,
        id: "session-1".into(),
        agent_id: "talos".into(),
        execution_node_id: "laptop-1".into(),
        tree_identity: SessionTreeIdentity::Primary,
        workspace_id: "default".into(),
        title: "Session".into(),
        user_id: "user-1".into(),
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
