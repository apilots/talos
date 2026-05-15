use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use talos_agent::{AgentImpl, AgentOrchestrator, OrchestratorRequest};
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Role, ToolStream,
    ToolStreamEvent, Usage,
};
use talos_tools::{Tool, ToolContext, ToolOutputPayload, ToolRegistry};
use talos_tools_policy::ToolPolicy;
use talos_types::talos::{
    TalosInputItem, TalosProvider, TalosSourceType, TalosTimelineDelta, TalosTimelineEventPayload,
    TalosTimelineItemKind,
};
use talos_types::{SessionContext, SessionState, TalosSession};
use tokio::sync::mpsc;

static SESSION_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn extract_tool_result_text(payload: &Value) -> Option<String> {
    let result = payload.get("result")?;
    result.as_str().map(str::to_string).or_else(|| {
        result
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn tool_message_reports_success(payload: &Value) -> bool {
    payload
        .get("ok")
        .and_then(|value| value.as_bool())
        .unwrap_or_else(|| payload.get("result").is_some())
}

#[derive(Default)]
struct IntegrationState {
    chat_round: AtomicUsize,
    tool_executions: AtomicUsize,
    saw_tool_result_message: AtomicBool,
    captured_tool_result: Mutex<Option<String>>,
}

struct EchoTool {
    state: Arc<IntegrationState>,
}

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_test"
    }

    fn description(&self) -> &str {
        "Echo message back for integration tests"
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": { "type": "string" }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        self.state.tool_executions.fetch_add(1, Ordering::SeqCst);
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("missing");
        Ok(ToolOutputPayload::success_text(format!("echo:{message}")))
    }
}

struct IntegrationMockProvider {
    state: Arc<IntegrationState>,
}

#[async_trait]
impl LlmProvider for IntegrationMockProvider {
    fn name(&self) -> &str {
        "integration-mock"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);

        if round == 0 {
            assert!(request
                .tools
                .as_ref()
                .is_some_and(|items| !items.is_empty()));
            return Ok(ChatResponse {
                content: "I should call a tool first.".to_string(),
                reasoning: Vec::new(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_1",
                    "echo_test",
                    json!({ "message": "ping" }),
                )]),
            });
        }

        if let Some(message) = request.messages.iter().find(|msg| {
            msg.role == Role::Tool
                && msg.name.as_deref() == Some("echo_test")
                && msg.tool_call_id.as_deref() == Some("call_1")
        }) {
            if let Ok(payload) = serde_json::from_str::<Value>(&message.content) {
                if tool_message_reports_success(&payload) {
                    if let Some(result) = extract_tool_result_text(&payload) {
                        self.state
                            .captured_tool_result
                            .lock()
                            .expect("captured_tool_result mutex poisoned")
                            .replace(result.to_string());
                        self.state
                            .saw_tool_result_message
                            .store(true, Ordering::SeqCst);
                    }
                }
            }
        }

        let tool_result = self
            .state
            .captured_tool_result
            .lock()
            .expect("captured_tool_result mutex poisoned")
            .clone()
            .unwrap_or_else(|| "missing-tool-result".to_string());

        Ok(ChatResponse {
            content: format!("Final answer after tool execution: {tool_result}"),
            reasoning: Vec::new(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = mpsc::channel(8);
        drop(tx);
        Ok(ChatStream { rx })
    }

    async fn stream_with_tools(&self, request: ChatRequest) -> TalosResult<ToolStream> {
        let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);
        if round > 0 {
            if let Some(message) = request.messages.iter().find(|msg| {
                msg.role == Role::Tool
                    && msg.name.as_deref() == Some("echo_test")
                    && msg.tool_call_id.as_deref() == Some("call_1")
            }) {
                if let Ok(payload) = serde_json::from_str::<Value>(&message.content) {
                    if tool_message_reports_success(&payload) {
                        if let Some(result) = extract_tool_result_text(&payload) {
                            self.state
                                .captured_tool_result
                                .lock()
                                .expect("captured_tool_result mutex poisoned")
                                .replace(result.to_string());
                            self.state
                                .saw_tool_result_message
                                .store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
        }

        let captured_result = self
            .state
            .captured_tool_result
            .lock()
            .expect("captured_tool_result mutex poisoned")
            .clone()
            .unwrap_or_else(|| "missing-tool-result".to_string());
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            if round == 0 {
                let _ = tx
                    .send(Ok(ToolStreamEvent::ReasoningDelta {
                        delta: "Need tool context first.".to_string(),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ToolStreamEvent::TextDelta {
                        delta: "Starting response. ".to_string(),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ToolStreamEvent::ToolCallStart {
                        index: 0,
                        id: Some("call_1".to_string()),
                        name: Some("echo_test".to_string()),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ToolStreamEvent::ToolCallDelta {
                        index: 0,
                        arguments_delta: "{\"message\":\"ping\"}".to_string(),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ToolStreamEvent::ToolCallEnd {
                        index: 0,
                        id: Some("call_1".to_string()),
                        name: Some("echo_test".to_string()),
                        arguments: json!({ "message": "ping" }),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ToolStreamEvent::Done {
                        finish_reason: Some("stop".to_string()),
                        usage: None,
                    }))
                    .await;
                return;
            }

            let _ = tx
                .send(Ok(ToolStreamEvent::ReasoningDelta {
                    delta: "Tool result received.".to_string(),
                }))
                .await;
            let _ = tx
                .send(Ok(ToolStreamEvent::TextDelta {
                    delta: format!("Final answer after tool execution: {}", captured_result),
                }))
                .await;
            let _ = tx
                .send(Ok(ToolStreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                    usage: None,
                }))
                .await;
        });

        Ok(ToolStream::new(rx))
    }
}

fn create_agent_with_tool(state: Arc<IntegrationState>) -> AgentImpl {
    let provider = Arc::new(IntegrationMockProvider {
        state: state.clone(),
    });
    let policy = ToolPolicy::allow_list(vec!["echo_test"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));
    registry.register(Arc::new(EchoTool {
        state: state.clone(),
    }));
    AgentImpl::new(
        "integration_agent".to_string(),
        "Integration Agent".to_string(),
        "Agent for integration tests".to_string(),
        provider,
        "mock-model".to_string(),
        registry,
        Default::default(),
    )
}

fn build_request(input: &str) -> OrchestratorRequest {
    let session_id = format!(
        "integration-session-{}",
        SESSION_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    OrchestratorRequest {
        session: TalosSession {
            title: "Untitled Session".to_string(),
            user_id: "integration-user".to_string(),
            conf: talos_types::SessionConf::default(),
            project_id: None,
            current_directory: None,
            context: SessionContext {
                session_id: session_id.clone(),
                workspace_id: Some("default".to_string()),
                project_id: None,
                current_directory: None,
                project_root: None,
                source_type: TalosSourceType::System,
                provider: TalosProvider::System,
                user_id: "integration-user".to_string(),
                message_id: "integration-message".to_string(),
                trace_id: "integration-trace".to_string(),
                channel_id: None,
                thread_id: None,
                environment: None,
            },
            config: talos_config::Config::default(),
            state: SessionState::default(),
        },
        turn_id: None,
        resume_context: None,
        input: vec![TalosInputItem::Text {
            text: input.to_string(),
        }],
    }
}

#[tokio::test]
async fn integration_tool_call_loop_executes_tool_and_returns_final_output() {
    let state = Arc::new(IntegrationState::default());
    let agent = create_agent_with_tool(state.clone());

    let response = agent
        .run(build_request("Please use your tool first and then answer."))
        .await
        .expect("agent should return a response");

    let captured_tool_result = state
        .captured_tool_result
        .lock()
        .expect("captured_tool_result mutex poisoned")
        .clone();
    assert_eq!(captured_tool_result.as_deref(), Some("echo:ping"));
    assert!(response
        .output_text
        .contains("Final answer after tool execution"));
    assert!(response.reasoning.is_empty());
    assert!(response.output_items.iter().any(|item| matches!(
        item,
        talos_types::talos::TalosOutputItem::ExecutionSummary { .. }
    )));
    assert!(state.saw_tool_result_message.load(Ordering::SeqCst));
    assert_eq!(state.tool_executions.load(Ordering::SeqCst), 1);
    assert!(state.chat_round.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn integration_stream_exposes_reasoning_output_and_tool_events() {
    let state = Arc::new(IntegrationState::default());
    let agent = create_agent_with_tool(state);

    let mut stream = agent
        .run_stream(build_request("Stream your response with tool events."))
        .await
        .expect("agent should return stream receiver");

    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            TalosTimelineEventPayload::TurnStarted(payload) if !payload.request_id.is_empty()
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            TalosTimelineEventPayload::ItemDelta(payload)
                if matches!(&payload.delta, TalosTimelineDelta::Text { text } if text == "Need tool context first.")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            TalosTimelineEventPayload::ItemDelta(payload)
                if matches!(&payload.delta, TalosTimelineDelta::Text { text } if text.contains("Final answer after tool execution"))
        )
    }));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        TalosTimelineEventPayload::ItemStarted(payload)
            if payload.item.kind == TalosTimelineItemKind::ToolCall
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        TalosTimelineEventPayload::ItemCompleted(payload)
            if payload.item_id.starts_with("tool-")
    )));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            TalosTimelineEventPayload::TurnCompleted(payload)
                if payload.stop_reason == talos_types::talos::StopReason::EndTurn
        )
    }));
}
