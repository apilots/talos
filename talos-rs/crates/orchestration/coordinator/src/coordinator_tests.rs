use super::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{
    AgentCapabilities, AgentDescriptor, AgentInputParams, AgentInputResult, AgentInterruptParams,
    AgentInterruptResult, AgentKind, AgentResult, AgentResumeParams, AgentRunParams,
    AgentRunResult, AgentStatus,
};
use talos_error::{TalosError, TalosResult};
use talos_types::agent_dispatch::{
    AgentDispatchInput, AgentDispatchInputParams, AgentDispatchParams, AgentDispatchResumeContext,
    AgentDispatchResumeToolCall,
};
use talos_types::talos::{
    AgentEvent, StopReason, TalosContext, TalosInputItem, TalosProvider, TalosSourceType,
    TokenUsage,
};
use talos_types::{
    ModelRef, ModelSelection, SessionConf, SessionContext, SessionState, TalosConfig, TalosSession,
    DEFAULT_SESSION_TITLE,
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
    capabilities: AgentCapabilities,
    run_requests: Arc<Mutex<Vec<AgentRunParams>>>,
    stream_requests: Arc<Mutex<Vec<AgentRunParams>>>,
    resume_requests: Arc<Mutex<Vec<AgentResumeParams>>>,
    resume_stream_requests: Arc<Mutex<Vec<AgentResumeParams>>>,
    interrupt_requests: Arc<Mutex<Vec<AgentInterruptParams>>>,
    input_requests: Arc<Mutex<Vec<AgentInputParams>>>,
}

impl RecordingStreamAgent {
    fn new() -> Self {
        Self::with_id("test_agent")
    }

    fn with_id(id: &str) -> Self {
        Self {
            id: id.to_string(),
            capabilities: AgentCapabilities::kernel(),
            run_requests: Arc::new(Mutex::new(Vec::new())),
            stream_requests: Arc::new(Mutex::new(Vec::new())),
            resume_requests: Arc::new(Mutex::new(Vec::new())),
            resume_stream_requests: Arc::new(Mutex::new(Vec::new())),
            interrupt_requests: Arc::new(Mutex::new(Vec::new())),
            input_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn with_capabilities(capabilities: AgentCapabilities) -> Self {
        Self {
            capabilities,
            ..Self::new()
        }
    }
}

fn test_session(session_id: &str, context: TalosContext) -> TalosSession {
    let config = TalosConfig::default();
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
        conf: SessionConf {
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: config.llm.provider.clone(),
                id: config.llm.model.clone(),
            })),
        },
        project_id: None,
        current_directory: None,
        config,
        state: SessionState {
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            usage: TokenUsage::default(),
        },
    }
}

fn test_context(message_id: &str) -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Http,
        provider: TalosProvider::Sdk,
        user_id: "user-1".to_string(),
        message_id: message_id.to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: Default::default(),
    }
}

fn test_dispatch_params(session_id: &str, turn_id: &str) -> AgentDispatchParams {
    let context = test_context("msg-dispatch");
    AgentDispatchParams {
        turn_id: turn_id.to_string(),
        session: test_session(session_id, context),
        resume_context: None,
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
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

    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id.clone(),
            name: self.id.clone(),
            kind: AgentKind::Kernel,
            description: String::new(),
            capabilities: self.capabilities.clone(),
            status: AgentStatus::Enabled,
        }
    }

    async fn run(&self, request: AgentRunParams) -> AgentResult<AgentRunResult> {
        let session_id = request.context.session_id.clone();
        self.run_requests.lock().unwrap().push(request);
        Ok(AgentRunResult {
            output_text: "ok".to_string(),
            output_items: Vec::new(),
            reasoning: Default::default(),
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 1,
            session_id,
            generated_session_title: Some("Meaningful Topic".to_string()),
        })
    }

    async fn run_streamed(
        &self,
        request: AgentRunParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        self.stream_requests.lock().unwrap().push(request);
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn resume(&self, params: AgentResumeParams) -> AgentResult<AgentRunResult> {
        let session_id = params.run.context.session_id.clone();
        self.resume_requests.lock().unwrap().push(params);
        Ok(AgentRunResult {
            output_text: "ok".to_string(),
            output_items: Vec::new(),
            reasoning: Default::default(),
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 1,
            session_id,
            generated_session_title: None,
        })
    }

    async fn resume_streamed(
        &self,
        params: AgentResumeParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        self.resume_stream_requests.lock().unwrap().push(params);
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(
        &self,
        _params: talos_agent::AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
    }

    async fn clear_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        false
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> AgentResult<AgentInterruptResult> {
        self.interrupt_requests.lock().unwrap().push(params);
        Ok(AgentInterruptResult { interrupted: false })
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_input(&self, params: AgentInputParams) -> AgentResult<AgentInputResult> {
        self.input_requests.lock().unwrap().push(params);
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
            turn_id: "turn-route".to_string(),
        },
    )
    .await
    .expect("cancel should forward to agent");

    let recorded = interrupts.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].session_id, "session-cancel-route");
    assert_eq!(recorded[0].turn_id, "turn-route");
}

#[tokio::test]
async fn handle_stream_rejects_agent_without_streaming_capability() {
    let coordinator = CoordinatorImpl::new(Arc::new(MockRouter::new()));
    let agent = Arc::new(RecordingStreamAgent::with_capabilities(AgentCapabilities {
        streaming: false,
        interrupt: true,
        dynamic_input: true,
    }));
    let stream_requests = agent.stream_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent should register");

    let error = match talos_types::AgentDispatchRuntime::handle_stream(
        &coordinator,
        test_dispatch_params("session-no-stream", "turn-no-stream"),
    )
    .await
    {
        Ok(_) => panic!("stream dispatch should fail before reaching agent"),
        Err(error) => error,
    };

    assert!(matches!(error, TalosError::ValidationError(_)));
    assert!(error.to_string().contains("streaming"));
    assert!(stream_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn handle_stream_rejects_resume_context_without_dynamic_input_capability() {
    let coordinator = CoordinatorImpl::new(Arc::new(MockRouter::new()));
    let agent = Arc::new(RecordingStreamAgent::with_capabilities(AgentCapabilities {
        streaming: true,
        interrupt: true,
        dynamic_input: false,
    }));
    let stream_requests = agent.stream_requests.clone();
    let resume_stream_requests = agent.resume_stream_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent should register");

    let mut params = test_dispatch_params("session-no-resume", "turn-no-resume");
    params.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "item-1".to_string(),
            call_id: "call-1".to_string(),
            tool_name: "Tool".to_string(),
        }),
    });
    let error = match talos_types::AgentDispatchRuntime::handle_stream(&coordinator, params).await {
        Ok(_) => panic!("resume stream dispatch should fail before reaching agent"),
        Err(error) => error,
    };

    assert!(matches!(error, TalosError::ValidationError(_)));
    assert!(error.to_string().contains("dynamic_input"));
    assert!(stream_requests.lock().unwrap().is_empty());
    assert!(resume_stream_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancel_active_turn_rejects_agent_without_interrupt_capability() {
    let coordinator = CoordinatorImpl::new(Arc::new(MockRouter::new()));
    let agent = Arc::new(RecordingStreamAgent::with_capabilities(AgentCapabilities {
        streaming: true,
        interrupt: false,
        dynamic_input: true,
    }));
    let interrupt_requests = agent.interrupt_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent should register");

    let context = test_context("msg-no-interrupt");
    let session = test_session("session-no-interrupt", context);
    let error = SessionRuntime::cancel_active_turn(
        &coordinator,
        talos_types::CancelActiveTurnParams {
            session,
            turn_id: "turn-no-interrupt".to_string(),
        },
    )
    .await
    .expect_err("cancel should fail before reaching agent");

    assert!(matches!(error, TalosError::ValidationError(_)));
    assert!(error.to_string().contains("interrupt"));
    assert!(interrupt_requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn submit_dynamic_tool_result_rejects_agent_without_dynamic_input_capability() {
    let coordinator = CoordinatorImpl::new(Arc::new(MockRouter::new()));
    let agent = Arc::new(RecordingStreamAgent::with_capabilities(AgentCapabilities {
        streaming: true,
        interrupt: true,
        dynamic_input: false,
    }));
    let input_requests = agent.input_requests.clone();
    coordinator
        .register_agent("test_agent", agent)
        .await
        .expect("agent should register");

    let context = test_context("msg-no-input");
    let session = test_session("session-no-input", context);
    let error = talos_types::AgentDispatchRuntime::submit_input(
        &coordinator,
        AgentDispatchInputParams {
            session,
            turn_id: "turn-no-input".to_string(),
            input: AgentDispatchInput::DynamicToolResult {
                call_id: "call-1".to_string(),
                tool_name: "Tool".to_string(),
                output: "output".to_string(),
                success: true,
            },
        },
    )
    .await
    .expect_err("input should fail before reaching agent");

    assert!(matches!(error, TalosError::ValidationError(_)));
    assert!(error.to_string().contains("dynamic_input"));
    assert!(input_requests.lock().unwrap().is_empty());
}

#[path = "tests/core.rs"]
mod core;
