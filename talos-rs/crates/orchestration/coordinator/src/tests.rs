use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{OrchestratorExecution, OrchestratorRequest, SessionTitleUpdate};
use talos_error::TalosResult;
use talos_types::talos::{
    StopReason, TalosConnectionMode, TalosContext, TalosInputItem, TalosProvider,
    TalosSessionEnvironment, TalosSourceType, TalosTimelineEvent, TalosUsage,
};
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectPatch, ProjectRecord,
    SessionConf, SessionPatch, SessionPatchAck, SessionQuery, SessionRecord, TalosConfig,
    TalosSession, DEFAULT_SESSION_TITLE,
};
use tokio::sync::mpsc;
use uuid::Uuid;

struct MockRouter;

impl MockRouter {
    fn new() -> Self {
        Self
    }
}

struct RecordingStreamAgent {
    stream_requests: Arc<Mutex<Vec<OrchestratorRequest>>>,
}

impl RecordingStreamAgent {
    fn new() -> Self {
        Self {
            stream_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

struct TitleTestSessionStore {
    sessions: Arc<Mutex<HashMap<String, SessionRecord>>>,
}

struct TrackingFlushAgent {
    pending: usize,
    drained: Arc<AtomicUsize>,
}

impl TrackingFlushAgent {
    fn new(pending: usize, drained: Arc<AtomicUsize>) -> Self {
        Self { pending, drained }
    }
}

impl TitleTestSessionStore {
    fn new(session: SessionRecord) -> Self {
        let mut sessions = HashMap::new();
        sessions.insert(session.id.clone(), session);
        Self {
            sessions: Arc::new(Mutex::new(sessions)),
        }
    }
}

#[async_trait::async_trait]
impl talos_router::Router for MockRouter {
    async fn route(&self, _channel: &str, _user_id: &str) -> TalosResult<Option<String>> {
        Ok(Some("test_agent".to_string()))
    }

    async fn add_binding(&self, _binding: talos_router::Binding) -> TalosResult<()> {
        Ok(())
    }

    async fn remove_binding(&self, _id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn list_bindings(&self) -> TalosResult<Vec<talos_router::Binding>> {
        Ok(vec![])
    }
}

#[async_trait::async_trait]
impl AgentOrchestrator for RecordingStreamAgent {
    fn id(&self) -> &str {
        "recording-stream"
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        Ok(OrchestratorExecution {
            output_text: "ok".to_string(),
            output_items: Vec::new(),
            reasoning: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            task_count: 1,
            session_id: request.session.context.session_id,
            session_title_update: Some(SessionTitleUpdate {
                title: "Meaningful Topic".to_string(),
            }),
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        self.stream_requests.lock().unwrap().push(request);
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, _session: TalosSession) -> TalosResult<TalosUsage> {
        Ok(TalosUsage::default())
    }

    async fn clear_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_memory_flushes(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    fn pending_memory_flushes(&self) -> usize {
        0
    }

    async fn has_pending_memory_flush(&self, _session_id: &str) -> bool {
        false
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        false
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl talos_session_store::SessionStore for TitleTestSessionStore {
    async fn create_session(
        &self,
        context: &TalosContext,
        _agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> TalosResult<SessionRecord> {
        let record = SessionRecord {
            id: "created-session".to_string(),
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: context.user_id.clone(),
            context: context.clone(),
            model: requested_model.unwrap_or(default_model).to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TalosUsage::default(),
            conf: conf.unwrap_or_default(),
            project_id,
            current_directory,
        };
        self.sessions
            .lock()
            .unwrap()
            .insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> TalosResult<SessionRecord> {
        self.create_session(
            context,
            agent_id,
            default_model,
            requested_model,
            None,
            dm_scope,
            project_id,
            current_directory,
        )
        .await
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(self.sessions.lock().unwrap().get(session_id).cloned())
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(self.sessions.lock().unwrap().values().cloned().collect())
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(self.sessions.lock().unwrap().len())
    }

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> TalosResult<SessionPatchAck> {
        let requested_model = patch.agent_model()?;
        let mut sessions = self.sessions.lock().unwrap();
        let Some(record) = sessions.get_mut(session_id) else {
            return Ok(SessionPatchAck { success: false });
        };
        if let Some(model) = requested_model {
            record.model = model;
        }
        if let Some(project_id_patch) = &patch.project_id {
            match project_id_patch {
                OptionalProjectIdPatch::Set { project_id } => {
                    record.project_id = Some(project_id.clone());
                }
                OptionalProjectIdPatch::Clear => {
                    record.project_id = None;
                }
            }
        }
        if let Some(current_directory_patch) = &patch.current_directory {
            match current_directory_patch {
                OptionalCurrentDirectoryPatch::Set { path } => {
                    record.current_directory = Some(path.clone());
                }
                OptionalCurrentDirectoryPatch::Clear => {
                    record.current_directory = None;
                }
            }
        }
        Ok(SessionPatchAck { success: true })
    }

    async fn reset_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(record) = sessions.get_mut(session_id) else {
            return Ok(SessionPatchAck { success: false });
        };
        record.title = DEFAULT_SESSION_TITLE.to_string();
        record.message_count = 0;
        record.usage = TalosUsage::default();
        Ok(SessionPatchAck { success: true })
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
        let removed = self.sessions.lock().unwrap().remove(session_id).is_some();
        Ok(SessionPatchAck { success: removed })
    }

    async fn update_session_title(&self, session_id: &str, title: &str) -> TalosResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(session_id) else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };
        session.title = title.to_string();
        Ok(())
    }

    async fn record_session_progress(
        &self,
        session_id: &str,
        message_delta: u64,
        usage: &TalosUsage,
        model: Option<&str>,
    ) -> TalosResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(session) = sessions.get_mut(session_id) else {
            return Err(talos_error::TalosError::ResourceNotFound(format!(
                "session {}",
                session_id
            )));
        };
        session.message_count = session.message_count.saturating_add(message_delta);
        session.usage.total_tokens = session
            .usage
            .total_tokens
            .saturating_add(usage.total_tokens);
        if let Some(model) = model {
            session.model = model.to_string();
        }
        Ok(())
    }

    async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
        Ok(Vec::new())
    }

    async fn get_project(&self, _project_id: &str) -> TalosResult<Option<ProjectRecord>> {
        Ok(None)
    }

    async fn create_project(
        &self,
        name: &str,
        directory: &std::path::PathBuf,
    ) -> TalosResult<ProjectRecord> {
        self.create_project_for_owner(name, directory, "").await
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &std::path::PathBuf,
        user_id: &str,
    ) -> TalosResult<ProjectRecord> {
        Ok(ProjectRecord {
            project_id: "project-test".to_string(),
            name: name.to_string(),
            user_id: user_id.to_string(),
            directory: directory.clone(),
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: &ProjectPatch,
    ) -> TalosResult<Option<ProjectRecord>> {
        Ok(None)
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl AgentOrchestrator for TrackingFlushAgent {
    fn id(&self) -> &str {
        "tracking-flush"
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        Ok(OrchestratorExecution {
            output_text: String::new(),
            output_items: Vec::new(),
            reasoning: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            task_count: 0,
            session_id: request.session.context.session_id,
            session_title_update: None,
        })
    }

    async fn run_stream(
        &self,
        _request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, _session: TalosSession) -> TalosResult<TalosUsage> {
        Ok(TalosUsage::default())
    }

    async fn clear_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_memory_flushes(&self, _timeout: Duration) -> TalosResult<()> {
        self.drained.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn pending_memory_flushes(&self) -> usize {
        self.pending
    }

    async fn has_pending_memory_flush(&self, _session_id: &str) -> bool {
        self.pending > 0
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        false
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[path = "tests/core.rs"]
mod core;
#[path = "tests/history_remote.rs"]
mod history_remote;
