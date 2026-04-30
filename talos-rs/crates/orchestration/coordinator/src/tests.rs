use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{OrchestratorEvent, OrchestratorExecution, OrchestratorRequest};
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, MemoryStore, TalosTurn};
use talos_session_store::SessionStore;
use talos_types::talos::{
    StopReason, TalosConnectionMode, TalosContentItem, TalosContext, TalosInputItem,
    TalosOutputItem, TalosProvider, TalosSessionEnvironment, TalosSourceType, TalosUsage,
};
use talos_types::{
    OptionalCurrentDirectoryPatch, OptionalProjectIdPatch, ProjectPatch, ProjectRecord,
    SessionConf, SessionPatch, SessionPatchAck, SessionQuery, SessionRecord, TalosConfig,
    TalosSession, DEFAULT_SESSION_TITLE,
};
use tokio::sync::mpsc;
use uuid::Uuid;

fn test_timestamp() -> serde_json::Value {
    serde_json::json!("2026-04-09T00:00:00Z")
}

struct MockRouter;

impl MockRouter {
    fn new() -> Self {
        Self
    }
}

struct RecordingApprovalAgent {
    approvals: Arc<Mutex<Vec<(String, String, String, bool, Option<String>)>>>,
    resumed_inputs: Arc<Mutex<Vec<Vec<TalosInputItem>>>>,
    cancelled_sessions: Arc<Mutex<Vec<String>>>,
    compacted_sessions: Arc<Mutex<Vec<String>>>,
    cleared_sessions: Arc<Mutex<Vec<String>>>,
}

impl RecordingApprovalAgent {
    fn new() -> Self {
        Self {
            approvals: Arc::new(Mutex::new(Vec::new())),
            resumed_inputs: Arc::new(Mutex::new(Vec::new())),
            cancelled_sessions: Arc::new(Mutex::new(Vec::new())),
            compacted_sessions: Arc::new(Mutex::new(Vec::new())),
            cleared_sessions: Arc::new(Mutex::new(Vec::new())),
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

struct GuardedSessionAgent {
    active_turn: bool,
    pending_flush: bool,
    cleared_sessions: Arc<Mutex<Vec<String>>>,
}

impl GuardedSessionAgent {
    fn new(active_turn: bool, pending_flush: bool) -> Self {
        Self {
            active_turn,
            pending_flush,
            cleared_sessions: Arc::new(Mutex::new(Vec::new())),
        }
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
impl AgentOrchestrator for RecordingApprovalAgent {
    fn id(&self) -> &str {
        "recording-approval"
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        self.resumed_inputs
            .lock()
            .unwrap()
            .push(request.input.clone());
        Ok(OrchestratorExecution {
            output_text: "ok".to_string(),
            output_items: Vec::new(),
            reasoning: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TalosUsage::default(),
            task_count: 1,
            session_id: request.session.context.session_id,
        })
    }

    async fn run_stream(
        &self,
        _request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, session: TalosSession) -> TalosResult<TalosUsage> {
        self.compacted_sessions
            .lock()
            .unwrap()
            .push(session.context.session_id);
        Ok(TalosUsage {
            input_tokens: 5,
            output_tokens: 3,
            total_tokens: 8,
            cache_creation_input_tokens: Some(2),
            cache_read_input_tokens: Some(4),
        })
    }

    async fn clear_session(&self, session: TalosSession) -> TalosResult<()> {
        self.cleared_sessions
            .lock()
            .unwrap()
            .push(session.context.session_id);
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

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.cancelled_sessions
            .lock()
            .unwrap()
            .push(session_id.to_string());
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        self.approvals.lock().unwrap().push((
            session_id.to_string(),
            call_id.to_string(),
            tool_name.to_string(),
            approved,
            reason,
        ));
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
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
        })
    }

    async fn run_stream(
        &self,
        _request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
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

    async fn submit_tool_approval(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _approved: bool,
        _reason: Option<String>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl AgentOrchestrator for GuardedSessionAgent {
    fn id(&self) -> &str {
        "guarded-session"
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
        })
    }

    async fn run_stream(
        &self,
        _request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, _session: TalosSession) -> TalosResult<TalosUsage> {
        Ok(TalosUsage::default())
    }

    async fn clear_session(&self, session: TalosSession) -> TalosResult<()> {
        self.cleared_sessions
            .lock()
            .unwrap()
            .push(session.context.session_id);
        Ok(())
    }

    async fn drain_memory_flushes(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    fn pending_memory_flushes(&self) -> usize {
        usize::from(self.pending_flush)
    }

    async fn has_pending_memory_flush(&self, _session_id: &str) -> bool {
        self.pending_flush
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        self.active_turn
    }

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_tool_approval(
        &self,
        _session_id: &str,
        _call_id: &str,
        _tool_name: &str,
        _approved: bool,
        _reason: Option<String>,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session_id: &str,
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
#[path = "tests/session_actions.rs"]
mod session_actions;
