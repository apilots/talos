use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use talos_agent::{
    empty_skill_roots_resolver, AgentHarness, AgentHarnessImpl, AgentHarnessParams, AgentRunConfig,
    AgentRunContext, AgentRunOptions, AgentRunParams, AgentRunSessionState,
};
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, LlmProviderRouter,
    ModelBlockCompleted, ModelBlockDelta, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, ReasoningContinuation, ReasoningOutput, Role, Usage,
};
use talos_memory::{MemoryScope, MemoryWriteBatch, MemoryWriter};
use talos_tools::{Tool, ToolContext, ToolOutputPayload, ToolRegistry};
use talos_tools_policy::ToolPolicy;
use talos_types::talos::{
    AgentEventPayload, TalosInputItem, TalosProvider, TalosSourceType, TalosTimelineDelta,
    TalosTimelineItemKind,
};
use talos_types::{ModelRef, ModelSelection, SessionContext, SessionState, TalosSession};
use tokio::sync::mpsc;

static SESSION_COUNTER: AtomicUsize = AtomicUsize::new(1);

struct TestMemoryWriter;

impl MemoryWriter for TestMemoryWriter {
    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _batch: MemoryWriteBatch,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = talos_error::TalosResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

fn extract_tool_result_text(payload: &Value) -> Option<String> {
    if payload.get("type").and_then(Value::as_str) == Some("text") {
        return payload
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string);
    }

    let result = payload.get("result")?;
    result.as_str().map(str::to_string).or_else(|| {
        result
            .get("result")
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

fn tool_message_reports_success(payload: &Value) -> bool {
    if payload.get("type").and_then(Value::as_str) == Some("text") {
        return true;
    }

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

struct StaticLlmProviderRouter {
    provider: Arc<dyn LlmProvider>,
}

impl LlmProviderRouter for StaticLlmProviderRouter {
    fn provider_for(&self, _model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        Ok(self.provider.clone())
    }
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
                reasoning: talos_llm_provider::ReasoningOutput::default(),
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
            reasoning: talos_llm_provider::ReasoningOutput::default(),
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

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let round = self.state.chat_round.fetch_add(1, Ordering::SeqCst);
        if round > 0 {
            let assistant_message = request
                .messages
                .iter()
                .find(|message| {
                    message.role == Role::Assistant
                        && message
                            .tool_calls
                            .as_ref()
                            .is_some_and(|calls| calls.iter().any(|call| call.id == "call_1"))
                })
                .expect("tool loop should replay the assistant tool-call message");
            assert_eq!(
                assistant_message.reasoning_continuation,
                Some(ReasoningContinuation::Text(
                    "Need tool context first.".to_string()
                ))
            );
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
                    .send(Ok(ModelBlockStreamEvent::BlockStarted(
                        ModelBlockStarted::reasoning(Some("reasoning-0".to_string())),
                    )))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockDelta {
                        model_item_id: "reasoning".to_string(),
                        delta: ModelBlockDelta::ReasoningText {
                            delta: "Need tool context first.".to_string(),
                        },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockCompleted(
                        ModelBlockCompleted {
                            model_item_id: "reasoning".to_string(),
                        },
                    )))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::ReasoningCompleted {
                        output: ReasoningOutput {
                            display_text: vec!["Need tool context first.".to_string()],
                            continuation: Some(ReasoningContinuation::Text(
                                "Need tool context first.".to_string(),
                            )),
                        },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockStarted(
                        ModelBlockStarted::assistant_text(None, None),
                    )))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockDelta {
                        model_item_id: "assistant_text".to_string(),
                        delta: ModelBlockDelta::AssistantText {
                            delta: "Starting response. ".to_string(),
                        },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockCompleted(
                        ModelBlockCompleted {
                            model_item_id: "assistant_text".to_string(),
                        },
                    )))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockStarted(
                        ModelBlockStarted::tool_call(
                            0,
                            Some("call_1".to_string()),
                            Some("echo_test".to_string()),
                        ),
                    )))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockDelta {
                        model_item_id: "tool_call_0".to_string(),
                        delta: ModelBlockDelta::ToolArguments {
                            delta: "{\"message\":\"ping\"}".to_string(),
                        },
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::BlockCompleted(
                        ModelBlockCompleted {
                            model_item_id: "tool_call_0".to_string(),
                        },
                    )))
                    .await;
                let _ = tx
                    .send(Ok(ModelBlockStreamEvent::Done {
                        finish_reason: Some("tool_calls".to_string()),
                        usage: None,
                    }))
                    .await;
                return;
            }

            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockStarted(
                    ModelBlockStarted::reasoning(Some("reasoning-1".to_string())),
                )))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "reasoning".to_string(),
                    delta: ModelBlockDelta::ReasoningText {
                        delta: "Tool result received.".to_string(),
                    },
                }))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockCompleted(
                    ModelBlockCompleted {
                        model_item_id: "reasoning".to_string(),
                    },
                )))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::ReasoningCompleted {
                    output: ReasoningOutput {
                        display_text: vec!["Tool result received.".to_string()],
                        continuation: Some(ReasoningContinuation::Text(
                            "Tool result received.".to_string(),
                        )),
                    },
                }))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockStarted(
                    ModelBlockStarted::assistant_text(None, None),
                )))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "assistant_text".to_string(),
                    delta: ModelBlockDelta::AssistantText {
                        delta: format!("Final answer after tool execution: {captured_result}"),
                    },
                }))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockCompleted(
                    ModelBlockCompleted {
                        model_item_id: "assistant_text".to_string(),
                    },
                )))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                    usage: None,
                }))
                .await;
        });

        Ok(ModelBlockStream::new(rx))
    }
}

fn create_agent_with_tool(state: Arc<IntegrationState>) -> AgentHarnessImpl {
    let provider = Arc::new(IntegrationMockProvider {
        state: state.clone(),
    });
    let policy = ToolPolicy::allow_list(vec!["echo_test"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));
    registry.register(Arc::new(EchoTool {
        state: state.clone(),
    }));
    AgentHarnessImpl::new(AgentHarnessParams {
        id: "integration_agent".to_string(),
        name: "Integration Agent".to_string(),
        description: "Agent for integration tests".to_string(),
        llm_provider_router: Arc::new(StaticLlmProviderRouter { provider }),
        model_selection: ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "mock-model".to_string(),
        }),
        tool_registry: registry,
        kernel_config: Default::default(),
        skill_roots_resolver: empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(talos_store::session::FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        memory_writer: Arc::new(TestMemoryWriter),
    })
    .expect("agent harness should build")
}

fn build_request(input: &str) -> AgentRunParams {
    let session_id = format!(
        "integration-session-{}",
        SESSION_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let turn_id = format!("turn-{session_id}");
    let mut config = talos_config::Config::default();
    config.llm.openai.models = vec![talos_config::ProviderModelConfig {
        id: "mock-model".to_string(),
        name: "mock-model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    let session = TalosSession {
        title: "Untitled Session".to_string(),
        user_id: "talos-test-integration-user".to_string(),
        conf: talos_types::SessionConf {
            model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                provider: "openai".to_string(),
                id: "mock-model".to_string(),
            })),
        },
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
            user_id: "talos-test-integration-user".to_string(),
            message_id: "integration-message".to_string(),
            trace_id: "integration-trace".to_string(),
            channel_id: None,
            thread_id: None,
            environment: None,
        },
        config,
        state: SessionState::default(),
    };
    let context = AgentRunContext::from_session(&session, turn_id);
    let config = AgentRunConfig::from_session(&session);
    let session_state = AgentRunSessionState::from_session(&session);
    let options = AgentRunOptions::from_config(&config);
    AgentRunParams::new(
        context,
        config,
        session_state,
        options,
        vec![TalosInputItem::Text {
            text: input.to_string(),
        }],
    )
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
    assert!(response
        .output_items
        .iter()
        .all(|item| !matches!(item, talos_types::talos::RunItem::ExecutionSummary { .. })));
    assert!(state.saw_tool_result_message.load(Ordering::SeqCst));
    assert_eq!(state.tool_executions.load(Ordering::SeqCst), 1);
    assert!(state.chat_round.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn integration_stream_exposes_reasoning_output_and_tool_events() {
    let state = Arc::new(IntegrationState::default());
    let agent = create_agent_with_tool(state);

    let mut stream = agent
        .run_streamed(build_request("Stream your response with tool events."))
        .await
        .expect("agent should return stream receiver");

    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }

    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            AgentEventPayload::TurnStarted(payload) if !payload.request_id.is_empty()
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            AgentEventPayload::ItemDelta(payload)
                if matches!(&payload.delta, TalosTimelineDelta::Text { text } if text == "Need tool context first.")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            AgentEventPayload::ItemDelta(payload)
                if matches!(&payload.delta, TalosTimelineDelta::Text { text } if text.contains("Final answer after tool execution"))
        )
    }));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemStarted(payload)
            if payload.item.kind == TalosTimelineItemKind::ToolCall
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id.starts_with("tool-")
    )));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            AgentEventPayload::TurnCompleted(payload)
                if payload.stop_reason == talos_types::talos::StopReason::EndTurn
        )
    }));
}
