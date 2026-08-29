use super::{Actor, CreateSessionRequest, ResourceAccessService};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use talos_coordinator::SessionRuntime;
use talos_error::TalosResult;
use talos_store::profile::MockProfileStore;
use talos_store::project::{ProjectRecord, ProjectStore};
use talos_store::session::{SessionQuery, SessionRecord, SessionStore, SessionSummary};
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};
use talos_types::{
    AgentTurnRef, ModelRef, ModelSelection, ProjectPatch, SessionConf, SessionListRequest,
    SessionPatch, TalosConfig, DEFAULT_SESSION_TITLE,
};

#[derive(Default)]
struct RecordingSessionStore {
    listed_queries: Mutex<Vec<SessionQuery>>,
    records: Mutex<Vec<SessionRecord>>,
    deleted_sessions: Mutex<Vec<String>>,
    projects: Mutex<Vec<ProjectRecord>>,
}

impl RecordingSessionStore {
    fn from_records(records: Vec<SessionRecord>) -> Self {
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
    let mut config = TalosConfig::default();
    config.llm.model = "default".to_string();
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: config.llm.model.clone(),
        name: "Default test model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    Arc::new(crate::session::SessionService::new(
        store.clone(),
        store,
        config,
    ))
}

fn recording_access_service(
    runtime: Arc<dyn SessionRuntime>,
    store: Arc<RecordingSessionStore>,
) -> ResourceAccessService {
    let sessions = recording_session_service(store.clone());
    let projects = Arc::new(crate::project::ProjectService::new(store));
    ResourceAccessService::with_services(
        runtime,
        sessions,
        projects,
        Arc::new(MockProfileStore::default()),
    )
}

#[async_trait]
impl SessionStore for RecordingSessionStore {
    async fn insert_session_record(&self, record: &SessionRecord, _scope: &str) -> TalosResult<()> {
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

    async fn get_subagent_record(
        &self,
        tree_root_session_id: &str,
        agent_path: &talos_types::AgentPath,
    ) -> TalosResult<Option<SessionRecord>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|record| match &record.tree_identity {
                talos_store::session::SessionTreeIdentity::Subagent {
                    tree_root_session_id: root,
                    path,
                    ..
                } => root == tree_root_session_id && path == agent_path,
                talos_store::session::SessionTreeIdentity::Primary => false,
            })
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
                None,
                None,
            ),
            record_for("other-user", "session-2", None, None),
        ])
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(2)
    }

    async fn summarize_sessions(&self, user_id: &str) -> TalosResult<SessionSummary> {
        let records = self.records.lock().unwrap();
        let (session_count, message_count) = records
            .iter()
            .filter(|record| record.user_id == user_id)
            .fold((0_u64, 0_u64), |(count, messages), record| {
                (count + 1, messages + record.message_count)
            });
        Ok(SessionSummary {
            session_count,
            message_count,
        })
    }

    async fn has_active_subagent_turn(&self, _tree_root_session_id: &str) -> TalosResult<bool> {
        Ok(false)
    }

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records.iter_mut().find(|stored| stored.id == record.id) else {
            return Ok(false);
        };
        *stored = record.clone();
        Ok(true)
    }

    async fn update_context_usage(
        &self,
        session_id: &str,
        context_usage: &talos_types::ContextUsage,
    ) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records.iter_mut().find(|stored| stored.id == session_id) else {
            return Ok(false);
        };
        stored.usage.context = Some(context_usage.clone());
        Ok(true)
    }

    async fn advance_memory_processed_until_seq(
        &self,
        session_id: &str,
        _expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        if let Some(record) = self
            .records
            .lock()
            .unwrap()
            .iter_mut()
            .find(|record| record.id == session_id)
        {
            record.memory_processed_until_seq = next_until_seq;
        }
        Ok(())
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

    async fn delete_session_tree(&self, tree_root_session_id: &str) -> TalosResult<bool> {
        self.delete_session_record(tree_root_session_id).await
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

    async fn count_projects_for_owner(&self, user_id: &str) -> TalosResult<u64> {
        Ok(self
            .projects
            .lock()
            .unwrap()
            .iter()
            .filter(|project| project.user_id == user_id)
            .count() as u64)
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
        _params: talos_coordinator::AgentSessionCompactionParams,
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
    ) -> TalosResult<talos_types::CancelActiveTurnOutcome> {
        unreachable!("not used in this test")
    }
}

fn record_for(
    user_id: &str,
    session_id: &str,
    project_id: Option<String>,
    current_directory: Option<PathBuf>,
) -> SessionRecord {
    let conf = SessionConf {
        model_selection: Some(ModelSelection::without_reasoning(ModelRef {
            provider: "anthropic".to_string(),
            id: "default".to_string(),
        })),
        ..SessionConf::default()
    };
    SessionRecord {
        id: session_id.to_string(),
        agent_id: "default".to_string(),
        tree_identity: talos_store::session::SessionTreeIdentity::Primary,
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
        latest_turn_seq: 0,
        memory_processed_until_seq: 0,
        usage: talos_types::SessionUsage::default(),
        conf,
        workspace_id: "default".to_string(),
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
    let store = Arc::new(RecordingSessionStore::from_records(vec![record_for(
        "other-user",
        "session-1",
        None,
        None,
    )]));
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store.clone());

    let read_error = access
        .get_session(&Actor::regular("member-user"), "session-1")
        .await
        .expect_err("other user session should be hidden");
    assert!(read_error.is_code(talos_error::ErrorCode::ResourceNotFound));

    let patch_error = access
        .patch_session(
            &Actor::regular("member-user"),
            "session-1",
            SessionPatch::default(),
        )
        .await
        .expect_err("other user session should not be patched");
    assert!(patch_error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn child_session_rejects_ordinary_external_access() {
    let mut child = record_for("member-user", "child-1", None, None);
    child.tree_identity = talos_store::session::SessionTreeIdentity::Subagent {
        parent: AgentTurnRef {
            session_id: "parent-1".to_string(),
            turn_id: "turn-parent".to_string(),
        },
        tree_root_session_id: "parent-1".to_string(),
        path: "/root/child_1".parse().unwrap(),
    };
    let store = Arc::new(RecordingSessionStore::from_records(vec![child]));
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store);
    let actor = Actor::regular("member-user");

    assert!(access
        .get_session(&actor, "child-1")
        .await
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(access
        .get_session(&actor, "child-1")
        .await
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn open_mode_actor_only_sees_anonymous_sessions() {
    let store = Arc::new(RecordingSessionStore::from_records(vec![
        record_for("anonymous", "session-anonymous", None, None),
        record_for("member-user", "session-member", None, None),
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
            CreateSessionRequest {
                session_id: "session-created".to_string(),
                context,
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
            },
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

#[tokio::test]
async fn existing_session_rejects_a_project_override() {
    let store = Arc::new(RecordingSessionStore::default());
    store
        .records
        .lock()
        .unwrap()
        .push(record_for("member-user", "session-existing", None, None));
    let access = recording_access_service(Arc::new(NoopSessionRuntime), store);
    let context = TalosContext {
        source_type: TalosSourceType::Http,
        provider: TalosProvider::Http,
        user_id: "member-user".to_string(),
        message_id: "message-2".to_string(),
        trace_id: "trace-2".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: None,
    };

    let error = access
        .get_or_create_session(
            &Actor::regular("member-user"),
            CreateSessionRequest {
                session_id: "session-existing".to_string(),
                context,
                model_selection: None,
                user_permission_mode: None,
                project_id: Some("project-2".to_string()),
                current_directory: None,
            },
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("project binding is immutable"));
}
