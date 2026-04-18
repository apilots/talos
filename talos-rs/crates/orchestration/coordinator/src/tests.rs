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
        Ok(ProjectRecord {
            project_id: "project-test".to_string(),
            name: name.to_string(),
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

#[tokio::test]
async fn test_coordinator_creation() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

    assert_eq!(coordinator.agents.read().await.len(), 0);
}

#[tokio::test]
async fn test_register_agent() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());

    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

    let mock_agent = Arc::new(talos_agent::MockAgent::new());
    let result = coordinator.register_agent("test_agent", mock_agent).await;

    assert!(result.is_ok());
    assert_eq!(coordinator.agents.read().await.len(), 1);
}

#[tokio::test]
async fn test_drain_memory_flushes_delegates_to_registered_agents() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());

    let drained = Arc::new(AtomicUsize::new(0));
    coordinator
        .register_agent(
            "agent-a",
            Arc::new(TrackingFlushAgent::new(1, drained.clone())),
        )
        .await
        .expect("first agent should register");
    coordinator
        .register_agent(
            "agent-b",
            Arc::new(TrackingFlushAgent::new(2, drained.clone())),
        )
        .await
        .expect("second agent should register");

    assert_eq!(coordinator.pending_memory_flushes().await, 3);
    coordinator
        .drain_memory_flushes(Duration::from_secs(1))
        .await
        .expect("coordinator should drain all agents");
    assert_eq!(drained.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_handle_stream_returns_orchestrator_events() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
    coordinator
        .register_agent("test_agent", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("agent registration should succeed");

    let mut stream = coordinator
        .handle_stream(GatewayRequest {
            session_id: "test-session".to_string(),
            workspace_id: None,
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
            context: TalosContext {
                source_type: TalosSourceType::Im,
                provider: TalosProvider::Telegram,
                user_id: "user-1".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test-channel".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
        })
        .await
        .expect("stream request should succeed");

    let mut event_types = Vec::new();
    while let Some(event) = stream.recv().await {
        event_types.push(event.event_type.clone());
        if event.event_type == talos_types::talos::TalosEventType::MessageCompleted {
            break;
        }
    }

    assert!(event_types.contains(&talos_types::talos::TalosEventType::MessageStarted));
    assert!(event_types.contains(&talos_types::talos::TalosEventType::MessageOutputDelta));
    assert!(event_types.contains(&talos_types::talos::TalosEventType::MessageToolCallQueued));
    assert!(event_types.contains(&talos_types::talos::TalosEventType::MessageToolCallExecuting));
    assert!(event_types.contains(&talos_types::talos::TalosEventType::MessageToolCallProgressDelta));
    assert!(event_types.contains(&talos_types::talos::TalosEventType::MessageCompleted));
}

#[tokio::test]
async fn test_submit_tool_approval_routes_to_session_agent() {
    let mock_router = Arc::new(MockRouter::new());
    let mock_session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, mock_session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let approvals = agent.approvals.clone();
    let resumed_inputs = agent.resumed_inputs.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    coordinator
        .submit_tool_approval(
            "session-1",
            "call-1",
            "Bash",
            false,
            Some("deny".to_string()),
        )
        .await
        .expect("tool approval should route successfully");

    let approvals = approvals.lock().unwrap();
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        approvals[0],
        (
            "session-1".to_string(),
            "call-1".to_string(),
            "Bash".to_string(),
            false,
            Some("deny".to_string()),
        )
    );
    drop(approvals);

    let resumed_inputs = resumed_inputs.lock().unwrap();
    assert_eq!(resumed_inputs.len(), 1);
    assert!(resumed_inputs[0].is_empty());
}

#[tokio::test]
async fn test_compact_session_routes_to_session_agent_and_records_usage() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-compact".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 2,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator =
        CoordinatorImpl::new(mock_router, session_store.clone(), TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let compacted_sessions = agent.compacted_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let session = coordinator
        .compact_session("session-compact")
        .await
        .expect("compact session should succeed");

    assert_eq!(
        compacted_sessions.lock().unwrap().as_slice(),
        &["session-compact".to_string()]
    );
    assert_eq!(session.state.message_count, 2);
    assert_eq!(session.state.usage.total_tokens, 8);
    assert_eq!(session.state.usage.cache_creation_input_tokens, Some(2));
    assert_eq!(session.state.usage.cache_read_input_tokens, Some(4));
}

#[tokio::test]
async fn test_clear_session_routes_to_session_agent_and_resets_session_state() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-clear".to_string(),
        title: "Old Title".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 6,
        usage: TalosUsage {
            input_tokens: 9,
            output_tokens: 3,
            total_tokens: 12,
            cache_creation_input_tokens: Some(1),
            cache_read_input_tokens: Some(2),
        },
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let cleared_sessions = agent.cleared_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let session = coordinator
        .clear_session("session-clear")
        .await
        .expect("clear session should succeed");

    assert_eq!(
        cleared_sessions.lock().unwrap().as_slice(),
        &["session-clear".to_string()]
    );
    assert_eq!(session.title, DEFAULT_SESSION_TITLE);
    assert_eq!(session.state.message_count, 0);
    assert_eq!(session.state.usage, TalosUsage::default());
}

#[tokio::test]
async fn test_delete_session_clears_agent_state_and_removes_record() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-delete".to_string(),
        title: "Delete Me".to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 3,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator =
        CoordinatorImpl::new(mock_router, session_store.clone(), TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let cleared_sessions = agent.cleared_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    let ack = coordinator
        .delete_session("session-delete")
        .await
        .expect("delete session should succeed");

    assert!(ack.success);
    assert_eq!(
        cleared_sessions.lock().unwrap().as_slice(),
        &["session-delete".to_string()]
    );
    assert!(coordinator
        .get_session("session-delete")
        .await
        .expect("get session should succeed")
        .is_none());
}

#[tokio::test]
async fn test_delete_session_rejects_active_turn() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-busy".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    coordinator
        .register_agent(
            "test_agent",
            Arc::new(GuardedSessionAgent::new(true, false)),
        )
        .await
        .expect("agent registration should succeed");

    let err = coordinator
        .delete_session("session-busy")
        .await
        .expect_err("delete should reject active session");
    assert!(matches!(err, TalosError::ValidationError(_)));
    assert!(err.to_string().contains("active"));
}

#[tokio::test]
async fn test_delete_session_rejects_pending_memory_flush() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-flushing".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    coordinator
        .register_agent(
            "test_agent",
            Arc::new(GuardedSessionAgent::new(false, true)),
        )
        .await
        .expect("agent registration should succeed");

    let err = coordinator
        .delete_session("session-flushing")
        .await
        .expect_err("delete should reject pending memory flush");
    assert!(matches!(err, TalosError::ValidationError(_)));
    assert!(err.to_string().contains("pending"));
}

#[tokio::test]
async fn test_first_turn_assigns_deterministic_session_title() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-1".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator =
        CoordinatorImpl::new(mock_router, session_store.clone(), TalosConfig::default());
    coordinator
        .register_agent("test_agent", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("agent registration should succeed");

    let response = coordinator
        .handle(GatewayRequest {
            session_id: "session-1".to_string(),
            workspace_id: None,
            input: vec![TalosInputItem::Text {
                text: "Fix tui paste handling".to_string(),
            }],
            context: TalosContext {
                source_type: TalosSourceType::Im,
                provider: TalosProvider::Telegram,
                user_id: "user-1".to_string(),
                message_id: "message-1".to_string(),
                trace_id: "trace-1".to_string(),
                channel_id: Some("test-channel".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
        })
        .await
        .expect("request should succeed");

    assert!(response.success);
    let updated = session_store
        .get_session_record("session-1")
        .await
        .expect("load session should succeed")
        .expect("session should exist");
    assert_eq!(updated.title, "Fix tui paste handling");
}

#[tokio::test]
async fn test_cancel_active_turn_routes_to_session_agent_and_is_noop_safe() {
    let mock_router = Arc::new(MockRouter::new());
    let session_record = SessionRecord {
        id: "session-cancel".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        context: TalosContext {
            source_type: TalosSourceType::Im,
            provider: TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("test-channel".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "test-model".to_string(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    let session_store = Arc::new(TitleTestSessionStore::new(session_record));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, TalosConfig::default());
    let agent = Arc::new(RecordingApprovalAgent::new());
    let cancelled_sessions = agent.cancelled_sessions.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent registration should succeed");

    talos_types::SessionService::cancel_active_turn(&coordinator, "session-cancel")
        .await
        .expect("cancel should route successfully");
    talos_types::SessionService::cancel_active_turn(&coordinator, "session-cancel")
        .await
        .expect("duplicate cancel should still be ok");

    let cancelled_sessions = cancelled_sessions.lock().unwrap();
    assert_eq!(
        cancelled_sessions.as_slice(),
        ["session-cancel".to_string(), "session-cancel".to_string()]
    );
}

#[tokio::test]
async fn test_get_session_turns_returns_ordered_items() {
    let session_id = format!("history-{}", Uuid::new_v4());
    let store = MemoryStore::new_default();
    let expected_items = vec![
        TalosOutputItem::UserInput {
            text: "user question".to_string(),
        },
        TalosOutputItem::Reasoning {
            text: "think first".to_string(),
        },
        TalosOutputItem::ToolCall {
            call_id: "call-1".to_string(),
            name: "Read".to_string(),
            arguments: serde_json::json!({"path":"/tmp/demo.txt"}),
        },
        TalosOutputItem::ToolResult {
            call_id: "call-1".to_string(),
            tool_name: "Read".to_string(),
            content: vec![TalosContentItem::Text {
                text: "demo".to_string(),
            }],
            is_error: false,
        },
        TalosOutputItem::OutputText {
            text: "final answer".to_string(),
        },
    ];
    let turn: TalosTurn = serde_json::from_value(serde_json::json!({
        "turn_id": format!("turn-{}", Uuid::new_v4()),
        "session_id": session_id,
        "seq": 1,
        "timestamp": test_timestamp(),
        "items": expected_items.clone(),
        "usage": {
            "input_tokens": 10,
            "output_tokens": 12,
            "total_tokens": 22
        },
        "stop_reason": "end_turn"
    }))
    .expect("turn should deserialize");
    store
        .append_turn(&turn)
        .await
        .expect("append turn should succeed");

    let coordinator = CoordinatorImpl::new(
        Arc::new(MockRouter::new()),
        Arc::new(talos_session_store::MockSessionStore::new()),
        TalosConfig::default(),
    );

    let turns = coordinator
        .get_session_turns(&session_id)
        .await
        .expect("session turns should load");

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].output.len(), expected_items.len());
    assert!(matches!(
        turns[0].output.first(),
        Some(TalosOutputItem::UserInput { text }) if text == "user question"
    ));
    assert!(matches!(
        turns[0].output.get(1),
        Some(TalosOutputItem::Reasoning { text }) if text == "think first"
    ));
    assert!(matches!(
        turns[0].output.get(2),
        Some(TalosOutputItem::ToolCall { name, .. }) if name == "Read"
    ));
    assert!(matches!(
        turns[0].output.get(3),
        Some(TalosOutputItem::ToolResult { tool_name, .. }) if tool_name == "Read"
    ));
    assert!(matches!(
        turns[0].output.get(4),
        Some(TalosOutputItem::OutputText { text }) if text == "final answer"
    ));
    assert_eq!(turns[0].usage.total_tokens, 22);
    assert_eq!(turns[0].stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn test_remote_session_defaults_project_root_to_workspace_dir() {
    let workspace_dir = std::env::temp_dir().join(format!("talos-coordinator-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");

    let mut config = TalosConfig::default();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mock_router = Arc::new(MockRouter::new());
    let session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, session_store, config);
    coordinator
        .register_agent("default", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("default agent should register");

    let session = talos_types::SessionService::create_session(
        &coordinator,
        TalosContext::new(
            TalosSourceType::Cli,
            TalosProvider::Cli,
            "user-1",
            "message-1",
            "trace-1",
        )
        .with_environment(TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/client/path"),
            connection_mode: TalosConnectionMode::Remote,
        }),
        None,
        None,
        None,
        None,
    )
    .await
    .expect("session creation should succeed");

    assert_eq!(
        session.context.project_root.as_deref(),
        Some(workspace_dir.as_path())
    );
}

#[tokio::test]
async fn test_remote_session_ignores_current_directory_patch_for_root_resolution() {
    let workspace_dir = std::env::temp_dir().join(format!("talos-coordinator-{}", Uuid::new_v4()));
    let custom_dir = std::env::temp_dir().join(format!("talos-project-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
    std::fs::create_dir_all(&custom_dir).expect("custom dir should be created");

    let mut config = TalosConfig::default();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mock_router = Arc::new(MockRouter::new());
    let session_store = Arc::new(TitleTestSessionStore::new(SessionRecord {
        id: "session-1".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        context: TalosContext::new(
            TalosSourceType::Cli,
            TalosProvider::Cli,
            "user-1",
            "message-1",
            "trace-1",
        )
        .with_environment(TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/client/path"),
            connection_mode: TalosConnectionMode::Remote,
        }),
        model: config.llm.model.clone(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: Some(custom_dir.clone()),
    }));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, config);

    talos_types::SessionService::patch_session(
        &coordinator,
        "session-1",
        SessionPatch {
            conf: None,
            project_id: None,
            current_directory: Some(OptionalCurrentDirectoryPatch::Set {
                path: custom_dir.clone(),
            }),
        },
    )
    .await
    .expect("patch should succeed");

    let session = talos_types::SessionService::get_session(&coordinator, "session-1")
        .await
        .expect("get session should succeed")
        .expect("session should exist");

    assert_eq!(
        session.context.project_root.as_deref(),
        Some(workspace_dir.as_path())
    );
}

#[tokio::test]
async fn test_get_session_turns_includes_tool_results() {
    let session_id = format!("history-tool-{}", Uuid::new_v4());
    let store = MemoryStore::new_default();
    let turn: TalosTurn = serde_json::from_value(serde_json::json!({
        "turn_id": format!("turn-{}", Uuid::new_v4()),
        "session_id": session_id,
        "seq": 1,
        "timestamp": test_timestamp(),
        "items": [
            {
                "type": "tool_call",
                "call_id": "call-tool",
                "name": "Bash",
                "arguments": {"command": "pwd"}
            },
            {
                "type": "tool_result",
                "call_id": "call-tool",
                "tool_name": "Bash",
                "content": [
                    {
                        "type": "json",
                        "value": {"output": "/tmp\n", "exit_code": 0}
                    }
                ],
                "is_error": false
            }
        ],
        "usage": {
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        },
        "stop_reason": "end_turn"
    }))
    .expect("turn should deserialize");
    store
        .append_turn(&turn)
        .await
        .expect("append turn should succeed");

    let coordinator = CoordinatorImpl::new(
        Arc::new(MockRouter::new()),
        Arc::new(talos_session_store::MockSessionStore::new()),
        TalosConfig::default(),
    );

    let turns = coordinator
        .get_session_turns(&session_id)
        .await
        .expect("session turns should load");

    assert!(matches!(
        turns[0].output.get(1),
        Some(TalosOutputItem::ToolResult { tool_name, .. }) if tool_name == "Bash"
    ));
}
