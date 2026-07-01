use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{
    AgentInputParams, AgentInputResult, AgentInterruptParams, AgentInterruptResult, AgentRunParams,
    AgentRunResult, SessionTitleUpdate,
};
use talos_error::TalosResult;
use talos_types::talos::{
    AgentEvent, StopReason, TalosContext, TalosInputItem, TalosProvider, TalosSourceType,
    TokenUsage,
};
use talos_types::{
    SessionConf, SessionContext, SessionState, TalosConfig, TalosSession, DEFAULT_SESSION_TITLE,
};
use tokio::sync::mpsc;

struct MockRouter {
    target_agent: Option<String>,
}

impl MockRouter {
    fn new() -> Self {
        Self {
            target_agent: Some("test_agent".to_string()),
        }
    }

    fn with_target(target_agent: Option<&str>) -> Self {
        Self {
            target_agent: target_agent.map(str::to_string),
        }
    }
}

struct RecordingStreamAgent {
    id: String,
    run_requests: Arc<Mutex<Vec<AgentRunParams>>>,
    stream_requests: Arc<Mutex<Vec<AgentRunParams>>>,
    interrupt_requests: Arc<Mutex<Vec<AgentInterruptParams>>>,
}

impl RecordingStreamAgent {
    fn new() -> Self {
        Self::with_id("test_agent")
    }

    fn with_id(id: &str) -> Self {
        Self {
            id: id.to_string(),
            run_requests: Arc::new(Mutex::new(Vec::new())),
            stream_requests: Arc::new(Mutex::new(Vec::new())),
            interrupt_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

struct TrackingFlushAgent {
    id: String,
    pending: usize,
    drained: Arc<AtomicUsize>,
}

impl TrackingFlushAgent {
    fn new(id: &str, pending: usize, drained: Arc<AtomicUsize>) -> Self {
        Self {
            id: id.to_string(),
            pending,
            drained,
        }
    }
}

fn test_session(session_id: &str, context: TalosContext) -> TalosSession {
    TalosSession {
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: context.user_id.clone(),
        context: SessionContext {
            session_id: session_id.to_string(),
            workspace_id: None,
            source_type: context.source_type,
            provider: context.provider,
            user_id: context.user_id,
            message_id: context.message_id,
            trace_id: context.trace_id,
            channel_id: context.channel_id,
            thread_id: context.thread_id,
            project_id: None,
            current_directory: None,
            project_root: context.project_root,
            environment: context.environment,
        },
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
        config: TalosConfig::default(),
        state: SessionState {
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TokenUsage::default(),
        },
    }
}

#[async_trait::async_trait]
impl talos_router::Router for MockRouter {
    async fn route(&self, _channel: &str, _user_id: &str) -> TalosResult<Option<String>> {
        Ok(self.target_agent.clone())
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
impl AgentHarness for RecordingStreamAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, request: AgentRunParams) -> TalosResult<AgentRunResult> {
        let session_id = request.session.context.session_id.clone();
        self.run_requests.lock().unwrap().push(request);
        Ok(AgentRunResult {
            output_text: "ok".to_string(),
            output_items: Vec::new(),
            reasoning: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 1,
            session_id,
            session_title_update: Some(SessionTitleUpdate {
                title: "Meaningful Topic".to_string(),
            }),
        })
    }

    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        self.stream_requests.lock().unwrap().push(request);
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, _session: TalosSession) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
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

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptResult> {
        self.interrupt_requests.lock().unwrap().push(params);
        Ok(AgentInterruptResult { interrupted: false })
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentInputResult> {
        Ok(AgentInputResult { accepted: true })
    }
}

#[async_trait::async_trait]
impl AgentHarness for TrackingFlushAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, request: AgentRunParams) -> TalosResult<AgentRunResult> {
        Ok(AgentRunResult {
            output_text: String::new(),
            output_items: Vec::new(),
            reasoning: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 0,
            session_id: request.session.context.session_id,
            session_title_update: None,
        })
    }

    async fn run_streamed(
        &self,
        _request: AgentRunParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, _session: TalosSession) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
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

    async fn interrupt(&self, _params: AgentInterruptParams) -> TalosResult<AgentInterruptResult> {
        Ok(AgentInterruptResult { interrupted: false })
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentInputResult> {
        Ok(AgentInputResult { accepted: true })
    }
}

#[tokio::test]
async fn cancel_active_turn_forwards_turn_id_to_agent() {
    let coordinator = CoordinatorImpl::new(Arc::new(MockRouter::new()));
    let agent = Arc::new(RecordingStreamAgent::new());
    let interrupts = agent.interrupt_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent should register");

    let session = test_session(
        "session-cancel-route",
        TalosContext {
            source_type: TalosSourceType::Http,
            provider: TalosProvider::Sdk,
            user_id: "user-1".to_string(),
            message_id: "msg-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_root: None,
            environment: Default::default(),
        },
    );

    SessionRuntime::cancel_active_turn(
        &coordinator,
        talos_types::CancelActiveTurnParams {
            session,
            turn_id: Some("turn-route".to_string()),
        },
    )
    .await
    .expect("cancel should forward to agent");

    let recorded = interrupts.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].session_id, "session-cancel-route");
    assert_eq!(recorded[0].turn_id.as_deref(), Some("turn-route"));
}

#[path = "tests/core.rs"]
mod core;
