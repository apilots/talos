use super::{Actor, ResourceAccessService};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use talos_coordinator::SessionRuntime;
use talos_error::TalosResult;
use talos_store::project::{ProjectRecord, ProjectStore};
use talos_store::session::{SessionQuery, SessionRecord, SessionStore};
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TokenUsage};
use talos_types::{
    ProjectPatch, SessionConf, SessionListRequest, SessionPatch, TalosConfig, DEFAULT_SESSION_TITLE,
};

#[derive(Default)]
struct RecordingSessionStore {
    listed_queries: Mutex<Vec<SessionQuery>>,
    records: Mutex<Vec<SessionRecord>>,
    deleted_sessions: Mutex<Vec<String>>,
    projects: Mutex<Vec<ProjectRecord>>,
}

impl RecordingSessionStore {
    fn with_records(records: Vec<SessionRecord>) -> Self {
        Self {
            listed_queries: Mutex::new(Vec::new()),
            records: Mutex::new(records),
            deleted_sessions: Mutex::new(Vec::new()),
            projects: Mutex::new(Vec::new()),
        }
    }
}

fn recording_session_service(
    store: Arc<RecordingSessionStore>,
) -> Arc<crate::session::SessionService> {
    Arc::new(crate::session::SessionService::new(
        store.clone(),
        store,
        TalosConfig::default(),
    ))
}

fn recording_access_service(
    runtime: Arc<dyn SessionRuntime>,
    store: Arc<RecordingSessionStore>,
) -> ResourceAccessService {
    let session_records = recording_session_service(store.clone());
    let projects = Arc::new(crate::project::ProjectService::new(store));
    ResourceAccessService::with_records(runtime, session_records, projects)
}

#[async_trait]
impl SessionStore for RecordingSessionStore {
    async fn insert_session_record(
        &self,
        record: &SessionRecord,
        _agent_id: &str,
        _scope: &str,
    ) -> TalosResult<()> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|record| record.id == session_id)
            .cloned())
    }

    async fn list_session_records(&self, query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        self.listed_queries.lock().unwrap().push(query.clone());
        let records = self.records.lock().unwrap();
        if !records.is_empty() {
            return Ok(records.clone());
        }
        Ok(vec![
            record_for(
                query.user_id.as_deref().unwrap_or("member-user"),
                "session-1",
                SessionConf::default(),
                None,
                None,
            ),
            record_for(
                "other-user",
                "session-2",
                SessionConf::default(),
                None,
                None,
            ),
        ])
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(2)
    }

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records.iter_mut().find(|stored| stored.id == record.id) else {
            return Ok(false);
        };
        *stored = record.clone();
        Ok(true)
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool> {
        self.deleted_sessions
            .lock()
            .unwrap()
            .push(session_id.to_string());
        let mut records = self.records.lock().unwrap();
        let len_before = records.len();
        records.retain(|record| record.id != session_id);
        Ok(records.len() != len_before)
    }
}

#[async_trait]
impl ProjectStore for RecordingSessionStore {
    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        Ok(self.projects.lock().unwrap().clone())
    }

    async fn get_project(&self, project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        Ok(self
            .projects
            .lock()
            .unwrap()
            .iter()
            .find(|project| project.project_id == project_id)
            .cloned())
    }

    async fn create_project(
        &self,
        _name: &str,
        _directory: &PathBuf,
    ) -> TalosResult<ProjectRecord> {
        unreachable!("not used in this test")
    }

    async fn create_project_for_owner(
        &self,
        _name: &str,
        _directory: &PathBuf,
        _user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        unreachable!("not used in this test")
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        unreachable!("not used in this test")
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
        unreachable!("not used in this test")
    }
}

struct NoopSessionRuntime;

#[async_trait]
impl SessionRuntime for NoopSessionRuntime {
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn compact_session(
        &self,
        _session: talos_types::TalosSession,
    ) -> TalosResult<talos_types::TokenUsage> {
        unreachable!("not used in this test")
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn prepare_delete_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(
        &self,
        _params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: talos_types::TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        unreachable!("not used in this test")
    }
}

#[derive(Default)]
struct RecordingSessionRuntime {
    prepared_deletes: Mutex<Vec<String>>,
}

#[async_trait]
impl SessionRuntime for RecordingSessionRuntime {
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn compact_session(
        &self,
        _session: talos_types::TalosSession,
    ) -> TalosResult<talos_types::TokenUsage> {
        unreachable!("not used in this test")
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn prepare_delete_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.prepared_deletes
            .lock()
            .unwrap()
            .push(session.context.session_id);
        Ok(())
    }

    async fn cancel_active_turn(
        &self,
        _params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: talos_types::TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        unreachable!("not used in this test")
    }
}

fn record_for(
    user_id: &str,
    session_id: &str,
    conf: SessionConf,
    project_id: Option<String>,
    current_directory: Option<PathBuf>,
) -> SessionRecord {
    SessionRecord {
        id: session_id.to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: user_id.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Http,
            provider: TalosProvider::Http,
            user_id: user_id.to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("web".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "default".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TokenUsage::default(),
        conf,
        workspace_id: None,
        project_id,
        current_directory,
    }
}

#[tokio::test]
async fn list_sessions_scopes_query_to_actor() {
    let store = Arc::new(RecordingSessionStore::default());
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store.clone());

    let sessions = access
        .list_sessions(
            &Actor::regular("member-user"),
            SessionListRequest::default(),
        )
        .await
        .expect("list_sessions should succeed");

    assert_eq!(sessions.len(), 1);
    assert_eq!(
        store.listed_queries.lock().unwrap()[0].user_id.as_deref(),
        Some("member-user")
    );
}

#[tokio::test]
async fn regular_actor_cannot_read_or_patch_other_user_session() {
    let store = Arc::new(RecordingSessionStore::with_records(vec![record_for(
        "other-user",
        "session-1",
        SessionConf::default(),
        None,
        None,
    )]));
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store.clone());

    let read_error = access
        .get_session(&Actor::regular("member-user"), "session-1")
        .await
        .expect_err("other user session should be hidden");
    assert!(matches!(
        read_error,
        talos_error::TalosError::ResourceNotFound(_)
    ));

    let patch_error = access
        .patch_session(
            &Actor::regular("member-user"),
            "session-1",
            SessionPatch::default(),
        )
        .await
        .expect_err("other user session should not be patched");
    assert!(matches!(
        patch_error,
        talos_error::TalosError::ResourceNotFound(_)
    ));
}

#[tokio::test]
async fn delete_session_prepares_runtime_then_deletes_record() {
    let store = Arc::new(RecordingSessionStore::with_records(vec![record_for(
        "member-user",
        "session-1",
        SessionConf::default(),
        None,
        None,
    )]));
    let runtime = Arc::new(RecordingSessionRuntime::default());
    let access = recording_access_service(runtime.clone(), store.clone());

    let ack = access
        .delete_session(&Actor::regular("member-user"), "session-1")
        .await
        .expect("delete_session should succeed");

    assert!(ack.success);
    assert_eq!(
        runtime.prepared_deletes.lock().unwrap().as_slice(),
        ["session-1"]
    );
    assert_eq!(
        store.deleted_sessions.lock().unwrap().as_slice(),
        ["session-1"]
    );
    assert!(store.records.lock().unwrap().is_empty());
}

#[tokio::test]
async fn open_mode_actor_only_sees_anonymous_sessions() {
    let store = Arc::new(RecordingSessionStore::with_records(vec![
        record_for(
            "anonymous",
            "session-anonymous",
            SessionConf::default(),
            None,
            None,
        ),
        record_for(
            "member-user",
            "session-member",
            SessionConf::default(),
            None,
            None,
        ),
    ]));
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store.clone());

    let sessions = access
        .list_sessions(&Actor::regular("anonymous"), SessionListRequest::default())
        .await
        .expect("list should succeed");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].context.session_id, "session-anonymous");
}

#[tokio::test]
async fn creating_session_uses_actor_user_id() {
    let store = Arc::new(RecordingSessionStore::default());
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store.clone());
    let context = TalosContext {
        source_type: TalosSourceType::Http,
        provider: TalosProvider::Http,
        user_id: "spoofed-user".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: None,
    };

    let session = access
        .get_or_create_session(
            &Actor::regular("member-user"),
            "session-created",
            context,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("session should be created");

    assert_eq!(session.user_id, "member-user");
    assert_eq!(session.context.user_id, "member-user");
    assert_eq!(
        store.records.lock().unwrap()[0].context.user_id,
        "member-user"
    );
}
