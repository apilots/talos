use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{AgentRequest, AgentResponse, SessionTitleUpdate};
use talos_error::TalosResult;
use talos_types::talos::{
    StopReason, TalosContext, TalosInputItem, TalosProvider, TalosSourceType, TalosTimelineEvent,
    TalosUsage,
};
use talos_types::{
    SessionConf, SessionContext, SessionState, TalosConfig, TalosSession, DEFAULT_SESSION_TITLE,
};
use tokio::sync::mpsc;

struct MockRouter;

impl MockRouter {
    fn new() -> Self {
        Self
    }
}

struct RecordingStreamAgent {
    stream_requests: Arc<Mutex<Vec<AgentRequest>>>,
}

impl RecordingStreamAgent {
    fn new() -> Self {
        Self {
            stream_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
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
            usage: TalosUsage::default(),
        },
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
impl Agent for RecordingStreamAgent {
    fn id(&self) -> &str {
        "recording-stream"
    }

    async fn run(&self, request: AgentRequest) -> TalosResult<AgentResponse> {
        Ok(AgentResponse {
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
        request: AgentRequest,
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
impl Agent for TrackingFlushAgent {
    fn id(&self) -> &str {
        "tracking-flush"
    }

    async fn run(&self, request: AgentRequest) -> TalosResult<AgentResponse> {
        Ok(AgentResponse {
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
        _request: AgentRequest,
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
