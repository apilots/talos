use super::*;
use crate::{
    AgentResumeParams, AgentRunConfig, AgentRunContext, AgentRunOptions, AgentRunSessionState,
};
use chrono::Utc;
use serde_json::json;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use talos_agent_kernel::{
    AssistantTextCompletedStreamData, AssistantTextDeltaStreamData, AssistantTextStartedStreamData,
    ContextCompactionStreamData, DynamicToolRequestStreamData, ExecutionSummaryStreamData,
    KernelAssistantTextPhase, KernelStreamEvent, KernelStreamEventData, KernelStreamEventType,
    KernelStreamItemKey, KernelThinkingSource, KernelThinkingStage, ThinkingDeltaStreamData,
    ToolArgsDeltaStreamData, ToolCallStartStreamData, ToolResultStreamData, ToolStreamKey,
};
use talos_config::{AgentKernelConfig, ProviderModelConfig};
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, ModelBlockCompleted,
    ModelBlockDelta, ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent,
    ReasoningContinuation, ReasoningOutput, Role, ToolDefinition, Usage,
};
use talos_memory::{MemoryScope, MemoryWriteBatch, MemoryWriter};
use talos_store::session::{self as session_artifacts, FileSessionArtifactStore};
use talos_tools::{
    default_governance_for_source, ReadTool, RegisteredToolSpec, ToolParallelism, ToolPayloadKind,
    ToolRegistry, ToolSource, WebSearchTool,
};
use talos_tools_policy::ToolPolicy;
use talos_types::agent_dispatch::{AgentDispatchResumeContext, AgentDispatchResumeToolCall};
use talos_types::talos::{
    AgentEventPayload, AgentEventType, ContextCompactionMetrics, ContextCompactionOutcome,
    ContextCompactionStage, ContextCompactionTrigger, StopReason as TalosStopReason,
    TalosContentItem, TalosInputItem, TalosProvider, TalosSourceType, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase,
    TokenUsage,
};
use talos_types::{
    ModelRef, ModelSelection, SessionConf, SessionContext, SessionState, TalosSession,
    DEFAULT_SESSION_TITLE,
};
use tempfile::TempDir;
use tokio::sync::Notify;

#[path = "compaction_tests.rs"]
mod compaction_tests;

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

fn test_agent_harness(
    id: String,
    name: String,
    description: String,
    llm_provider_router: Arc<dyn LlmProviderRouter>,
    model_selection: ModelSelection,
    tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
) -> talos_error::TalosResult<AgentHarnessImpl> {
    AgentHarnessImpl::new(AgentHarnessParams {
        id,
        name,
        description,
        llm_provider_router,
        model_selection,
        tool_registry,
        kernel_config,
        skill_roots_resolver: crate::empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        memory_writer: Arc::new(TestMemoryWriter),
    })
}

async fn append_transcript_record(
    workspace_root: &std::path::Path,
    record: &session_artifacts::SessionTranscriptRecord,
) -> talos_error::TalosResult<()> {
    FileSessionArtifactStore::default()
        .append_transcript_record(workspace_root, record)
        .await
}

async fn load_transcript_records(
    workspace_root: &std::path::Path,
    session_id: &str,
    invocation_id: Option<&str>,
    branch: session_artifacts::SessionTranscriptBranch,
) -> talos_error::TalosResult<Vec<session_artifacts::SessionTranscriptRecord>> {
    FileSessionArtifactStore::default()
        .load_transcript_records(workspace_root, session_id, invocation_id, Some(branch))
        .await
}

#[tokio::test]
async fn transcript_service_restores_session_state_without_a_filter() {
    let workspace = TempDir::new().expect("workspace");
    let service = crate::session_artifacts::SessionStoreKernelTranscriptService::new(
        Arc::new(FileSessionArtifactStore::default()),
        workspace.path().to_path_buf(),
    );
    let mut session = talos_agent_kernel::InvocationSession {
        session_id: "session-state".to_string(),
        ..talos_agent_kernel::InvocationSession::default()
    };
    let mut event = talos_agent_kernel::KernelTranscriptEvent::new(
        "session-state".to_string(),
        "turn-state".to_string(),
        talos_agent_kernel::ExecutionBranch::ModelThinking,
        talos_agent_kernel::EventAuthor::System,
        talos_agent_kernel::KernelTranscriptEventKind::Planning,
        talos_agent_kernel::KernelTranscriptPayload::StateOnly,
    );
    event
        .actions
        .state_delta
        .push(talos_agent_kernel::StateDelta {
            namespace: talos_agent_kernel::StateNamespace::Session,
            key: "context_compaction".to_string(),
            value: json!({"mode": "recovery"}),
        });
    talos_agent_kernel::KernelTranscriptService::append_event(&service, &mut session, event)
        .await
        .expect("state event should append");

    let restored = talos_agent_kernel::KernelTranscriptService::load_session(
        &service,
        "session-state",
        None,
        None,
    )
    .await
    .expect("session state should load");

    assert_eq!(
        restored.state.get(
            talos_agent_kernel::StateNamespace::Session,
            "context_compaction"
        ),
        Some(&json!({"mode": "recovery"}))
    );
}

fn build_test_agent() -> AgentHarnessImpl {
    test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(MockLlmProvider {})),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build")
}

struct StaticLlmProviderRouter {
    provider: Arc<dyn LlmProvider>,
}

impl LlmProviderRouter for StaticLlmProviderRouter {
    fn provider_for(
        &self,
        _model_ref: &ModelRef,
    ) -> talos_error::TalosResult<Arc<dyn LlmProvider>> {
        Ok(self.provider.clone())
    }
}

fn static_router(provider: Arc<dyn LlmProvider>) -> Arc<dyn LlmProviderRouter> {
    Arc::new(StaticLlmProviderRouter { provider })
}

fn model_selection(id: &str) -> ModelSelection {
    ModelSelection::without_reasoning(ModelRef {
        provider: "openai".to_string(),
        id: id.to_string(),
    })
}

fn provider_model(id: &str) -> ProviderModelConfig {
    ProviderModelConfig {
        id: id.to_string(),
        name: id.to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }
}

async fn submit_dynamic_tool_result(
    agent: &AgentHarnessImpl,
    session: TalosSession,
    call_id: &str,
    tool_name: &str,
    output: &str,
    success: bool,
) -> AgentResult<AgentInputResult> {
    agent
        .submit_input(AgentInputParams {
            session,
            turn_id: "turn-input".to_string(),
            input: AgentInput::DynamicToolResult {
                call_id: call_id.to_string(),
                tool_name: tool_name.to_string(),
                output: output.to_string(),
                success,
            },
        })
        .await
}

async fn interrupt_session(
    agent: &AgentHarnessImpl,
    session_id: &str,
) -> AgentResult<AgentInterruptResult> {
    agent
        .interrupt(AgentInterruptParams {
            session_id: session_id.to_string(),
            turn_id: "turn-active".to_string(),
        })
        .await
}

struct SequencedProvider {
    started: Arc<Notify>,
    calls: AtomicUsize,
}

struct CapturingMaxTokensProvider {
    seen_max_tokens: Arc<Mutex<Option<u32>>>,
    seen_model: Arc<Mutex<Option<String>>>,
}

struct DynamicResumeAgentProvider {
    calls: AtomicUsize,
}

struct NamedRecordingProvider {
    provider_name: &'static str,
    seen_provider: Arc<Mutex<Option<String>>>,
    seen_model: Arc<Mutex<Option<String>>>,
}

#[async_trait::async_trait]
impl LlmProvider for NamedRecordingProvider {
    fn name(&self) -> &str {
        self.provider_name
    }

    fn default_model(&self) -> String {
        "default-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        *self.seen_provider.lock().expect("provider lock poisoned") =
            Some(self.provider_name.to_string());
        *self.seen_model.lock().expect("model lock poisoned") = Some(request.model.clone());
        Ok(ChatResponse {
            content: "provider selected".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }

    async fn stream_model_blocks(
        &self,
        request: ChatRequest,
    ) -> talos_error::TalosResult<ModelBlockStream> {
        *self.seen_provider.lock().expect("provider lock poisoned") =
            Some(self.provider_name.to_string());
        *self.seen_model.lock().expect("model lock poisoned") = Some(request.model.clone());
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockStarted(
                    ModelBlockStarted::assistant_text(None, None),
                )))
                .await;
            let _ = tx
                .send(Ok(ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "assistant_text".to_string(),
                    delta: ModelBlockDelta::AssistantText {
                        delta: "provider selected".to_string(),
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

struct RecordingLlmProviderRouter {
    primary: Arc<dyn LlmProvider>,
    secondary: Arc<dyn LlmProvider>,
}

impl LlmProviderRouter for RecordingLlmProviderRouter {
    fn provider_for(&self, model_ref: &ModelRef) -> talos_error::TalosResult<Arc<dyn LlmProvider>> {
        if model_ref.provider == "deepseek" {
            Ok(self.secondary.clone())
        } else {
            Ok(self.primary.clone())
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for SequencedProvider {
    fn name(&self) -> &str {
        "sequenced"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> talos_error::TalosResult<ChatResponse> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            self.started.notify_one();
            pending::<talos_error::TalosResult<ChatResponse>>().await?;
            unreachable!("pending future should not resolve");
        }

        Ok(ChatResponse {
            content: "provider ok".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }

    async fn stream_model_blocks(
        &self,
        _request: ChatRequest,
    ) -> talos_error::TalosResult<ModelBlockStream> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            self.started.notify_one();
            pending::<talos_error::TalosResult<ModelBlockStream>>().await?;
            unreachable!("pending future should not resolve");
        }

        Ok(single_text_block_stream("provider ok"))
    }
}

#[async_trait::async_trait]
impl LlmProvider for CapturingMaxTokensProvider {
    fn name(&self) -> &str {
        "capturing-max-tokens"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> talos_error::TalosResult<ChatResponse> {
        let mut seen = self.seen_max_tokens.lock().expect("capture lock poisoned");
        if seen.is_none() {
            *seen = request.max_tokens;
        }
        let mut seen_model = self.seen_model.lock().expect("model capture lock poisoned");
        if seen_model.is_none() {
            *seen_model = Some(request.model.clone());
        }
        Ok(ChatResponse {
            content: "captured".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }
}

#[async_trait::async_trait]
impl LlmProvider for DynamicResumeAgentProvider {
    fn name(&self) -> &str {
        "dynamic-resume-agent"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(
        &self,
        request: ChatRequest,
    ) -> talos_error::TalosResult<ChatResponse> {
        self.assert_replayed_tool_message(&request);
        Ok(ChatResponse {
            content: "Final answer after dynamic pong.".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }

    async fn stream_model_blocks(
        &self,
        request: ChatRequest,
    ) -> talos_error::TalosResult<ModelBlockStream> {
        self.assert_replayed_tool_message(&request);
        Ok(single_text_block_stream("Final answer after dynamic pong."))
    }
}

impl DynamicResumeAgentProvider {
    fn assert_replayed_tool_message(&self, request: &ChatRequest) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let tool_message = request
            .messages
            .iter()
            .find(|message| {
                message.role == Role::Tool
                    && message.tool_call_id.as_deref() == Some("call-dynamic")
                    && message.name.as_deref() == Some("echo_dynamic")
            })
            .expect("dynamic resume should replay tool result into model request");
        assert!(
            tool_message.content.contains("dynamic pong"),
            "unexpected replayed tool message content: {}",
            tool_message.content
        );
        let tool_call = request
            .messages
            .iter()
            .filter(|message| message.role == Role::Assistant)
            .filter_map(|message| message.tool_calls.as_ref())
            .flatten()
            .find(|tool_call| tool_call.id == "call-dynamic" && tool_call.name == "echo_dynamic")
            .expect("dynamic resume should replay original dynamic request");
        assert_eq!(tool_call.arguments, json!({ "message": "ping" }));
        let assistant_message = request
            .messages
            .iter()
            .find(|message| {
                message.role == Role::Assistant
                    && message
                        .tool_calls
                        .as_ref()
                        .is_some_and(|calls| calls.iter().any(|call| call.id == "call-dynamic"))
            })
            .expect("dynamic resume should replay the assistant tool-call message");
        assert_eq!(
            assistant_message.reasoning_continuation,
            Some(ReasoningContinuation::Text(
                "dynamic provider continuation".to_string()
            ))
        );
    }
}

fn single_text_block_stream(text: &str) -> ModelBlockStream {
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let text = text.to_string();
    tokio::spawn(async move {
        let _ = tx
            .send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(None, None),
            )))
            .await;
        let _ = tx
            .send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText { delta: text },
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
    ModelBlockStream::new(rx)
}

fn build_temp_agent() -> (TempDir, AgentHarnessImpl) {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let kernel_config = AgentKernelConfig {
        workspace_dir: temp_dir.path().display().to_string(),
        ..Default::default()
    };
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(MockLlmProvider {})),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    )
    .expect("agent harness should build");
    (temp_dir, agent)
}

fn build_test_session(session_id: &str, workspace_id: &str) -> TalosSession {
    let mut config = talos_config::Config::default();
    config.llm.openai.models = [
        "gpt-4o",
        "agent-model",
        "session-model",
        "mock-model",
        "primary-model",
    ]
    .into_iter()
    .map(provider_model)
    .collect();
    config.llm.deepseek.models = vec![provider_model("secondary-model")];
    TalosSession {
        title: "Untitled Session".to_string(),
        user_id: "talos-test-local-user".to_string(),
        conf: SessionConf {
            model_selection: Some(model_selection("gpt-4o")),
        },
        project_id: None,
        current_directory: None,
        context: SessionContext {
            session_id: session_id.to_string(),
            workspace_id: Some(workspace_id.to_string()),
            project_id: None,
            current_directory: None,
            project_root: None,
            source_type: TalosSourceType::Cli,
            provider: TalosProvider::Cli,
            user_id: "talos-test-local-user".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            environment: None,
        },
        config,
        state: SessionState {
            created_at: 1,
            last_activity: 1,
            message_count: 2,
            usage: TokenUsage::default(),
        },
    }
}

fn agent_run_params(
    session: TalosSession,
    turn_id: &str,
    input: Vec<TalosInputItem>,
) -> AgentRunParams {
    let context = AgentRunContext::from_session(&session, turn_id.to_string());
    let config = AgentRunConfig::from_session(&session);
    let session_state = AgentRunSessionState::from_session(&session);
    let options = AgentRunOptions::from_config(&config);
    AgentRunParams::new(context, config, session_state, options, input)
}

#[test]
fn agent_run_config_resolves_deepseek_provider_config() {
    let mut llm = talos_config::LlmConfig::default();
    llm.deepseek.base_url = Some("https://api.deepseek.example".to_string());
    let config = AgentRunConfig {
        agent: talos_config::AgentConfig::default(),
        llm,
        model_selection: ModelSelection::without_reasoning(ModelRef {
            provider: "deepseek".to_string(),
            id: "deepseek-v4-pro".to_string(),
        }),
    };

    assert_eq!(
        config
            .current_provider_config()
            .and_then(|provider| provider.base_url.as_deref()),
        Some("https://api.deepseek.example")
    );
}

#[test]
fn test_tool_call_start_reuses_item_created_by_argument_delta() {
    let mut timeline = TimelineBuilder::new(
        "request-tool-stream",
        "turn-tool-stream",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();
    let stream_key = ToolStreamKey { round: 0, index: 0 };

    let argument_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolArgsDelta,
            KernelStreamEventData::ToolArgsDelta(ToolArgsDeltaStreamData {
                stream_key: stream_key.clone(),
                call_id: Some("call-read".to_string()),
                tool_name: Some("Read".to_string()),
                delta: "{\"path\":\"notes/read_target.txt\"}".to_string(),
            }),
        ),
    )
    .expect("tool argument delta should create a tool item");
    let tool_item_id = argument_events
        .iter()
        .find_map(|event| match &event.payload {
            AgentEventPayload::ItemStarted(payload) => Some(payload.item.item_id.clone()),
            _ => None,
        })
        .expect("tool item should be started");

    let start_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolCallStart,
            KernelStreamEventData::ToolCallStart(ToolCallStartStreamData {
                index: stream_key.index,
                call_id: Some("call-read".to_string()),
                tool_name: Some("Read".to_string()),
                arguments: json!({"path": "notes/read_target.txt"}),
                round: stream_key.round,
            }),
        ),
    )
    .expect("tool call start should reuse the existing tool item");

    assert!(
        start_events.is_empty(),
        "tool call start must not create a second timeline item"
    );
    assert_eq!(
        state.tool_item_by_call_id.get("call-read"),
        Some(&tool_item_id)
    );
    assert_eq!(
        timeline
            .item(&tool_item_id)
            .expect("tool item should exist")
            .status,
        TalosTimelineItemStatus::Running
    );
}

#[test]
fn test_assistant_text_commentary_maps_without_final_answer() {
    let mut timeline = TimelineBuilder::new(
        "request-commentary",
        "turn-commentary",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();
    let item_key = KernelStreamItemKey {
        round: Some(1),
        local_id: "assistant_text".to_string(),
    };

    let started_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key: item_key.clone(),
                phase: KernelAssistantTextPhase::Commentary,
                round: Some(1),
            }),
        ),
    )
    .expect("commentary should start");
    let item_id = match &started_events[0].payload {
        AgentEventPayload::ItemStarted(payload) => {
            assert_eq!(payload.item.kind, TalosTimelineItemKind::AssistantText);
            assert_eq!(payload.item.phase, TalosTimelinePhase::Commentary);
            payload.item.item_id.clone()
        }
        _ => panic!("expected commentary item start"),
    };

    let delta_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextDelta,
            KernelStreamEventData::AssistantTextDelta(AssistantTextDeltaStreamData {
                item_key: item_key.clone(),
                delta: "I will inspect the code.".to_string(),
            }),
        ),
    )
    .expect("commentary delta should append");
    assert!(delta_events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemDelta(payload)
            if payload.item_id == item_id
                && matches!(&payload.delta, TalosTimelineDelta::Text { text } if text == "I will inspect the code.")
    )));

    let completed_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextCompleted,
            KernelStreamEventData::AssistantTextCompleted(AssistantTextCompletedStreamData {
                item_key,
            }),
        ),
    )
    .expect("commentary should complete");
    assert!(completed_events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id == item_id
                && payload.status == TalosTimelineItemStatus::Completed
    )));
    assert!(state.final_answer_item_id.is_none());
}

#[test]
fn test_dynamic_request_marks_timeline_item_waiting_dynamic_input() {
    let mut timeline =
        TimelineBuilder::new("request-dynamic", "turn-dynamic", "message-1", "session-1");
    let mut state = KernelTimelineState::default();
    let start_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolCallStart,
            KernelStreamEventData::ToolCallStart(ToolCallStartStreamData {
                index: 0,
                call_id: Some("call-dynamic".to_string()),
                tool_name: Some("echo_test".to_string()),
                arguments: json!({"message": "ping"}),
                round: 0,
            }),
        ),
    )
    .expect("tool call start should emit timeline events");
    let tool_item_id = match &start_events[0].payload {
        AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
        _ => panic!("expected tool item start"),
    };

    let dynamic_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::DynamicToolRequest,
            KernelStreamEventData::DynamicToolRequest(DynamicToolRequestStreamData {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_test".to_string(),
                arguments: json!({"message": "ping"}),
            }),
        ),
    )
    .expect("dynamic request should update the existing tool item");

    assert!(dynamic_events.iter().any(|event| match &event.payload {
        AgentEventPayload::ItemDelta(payload) => {
            payload.item_id == tool_item_id
                && matches!(
                    payload.delta,
                    TalosTimelineDelta::Status {
                        status: TalosTimelineItemStatus::WaitingDynamicInput
                    }
                )
        }
        _ => false,
    }));
    assert!(dynamic_events.iter().any(|event| match &event.payload {
        AgentEventPayload::ItemDelta(payload) => {
            payload.item_id == tool_item_id
                && matches!(payload.delta, TalosTimelineDelta::ToolDynamicRequest { .. })
        }
        _ => false,
    }));
    assert_eq!(
        timeline
            .item(&tool_item_id)
            .expect("tool item should exist")
            .status,
        TalosTimelineItemStatus::WaitingDynamicInput
    );
}

#[test]
fn test_execution_summary_item_does_not_gate_final_answer_merge() {
    let mut timeline = TimelineBuilder::new(
        "request-summary-final",
        "turn-summary-final",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();

    let summary_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ExecutionSummary,
            KernelStreamEventData::ExecutionSummary(ExecutionSummaryStreamData {
                text: "Execution summary.".to_string(),
                round: None,
            }),
        ),
    )
    .expect("summary should map");
    let summary_id = match &summary_events[0].payload {
        AgentEventPayload::ItemStarted(payload) => {
            assert_eq!(payload.item.kind, TalosTimelineItemKind::ExecutionSummary);
            assert_eq!(payload.item.phase, TalosTimelinePhase::System);
            payload.item.item_id.clone()
        }
        _ => panic!("expected summary item start"),
    };
    assert!(matches!(
        &summary_events[1].payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id == summary_id
                && payload.status == TalosTimelineItemStatus::Completed
    ));

    let item_key = KernelStreamItemKey {
        round: None,
        local_id: "final".to_string(),
    };
    let final_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key,
                phase: KernelAssistantTextPhase::FinalAnswer,
                round: None,
            }),
        ),
    )
    .expect("final answer should start");

    assert!(summary_events[1].sequence < final_events[0].sequence);
}
#[test]
fn test_timeline_rejects_main_events_after_final_answer_started() {
    let mut timeline = TimelineBuilder::new(
        "request-post-final",
        "turn-post-final",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();
    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ExecutionSummary,
            KernelStreamEventData::ExecutionSummary(ExecutionSummaryStreamData {
                text: "Legacy execution summary.".to_string(),
                round: None,
            }),
        ),
    )
    .expect("summary should map");
    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key: KernelStreamItemKey {
                    round: None,
                    local_id: "final".to_string(),
                },
                phase: KernelAssistantTextPhase::FinalAnswer,
                round: None,
            }),
        ),
    )
    .expect("final answer should start");

    let err = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ThinkingDelta,
            KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                delta: "late reasoning".to_string(),
                source: KernelThinkingSource::Provider,
                stage: KernelThinkingStage::ModelReasoning,
                round: None,
            }),
        ),
    )
    .expect_err("thinking after final answer should fail");

    assert!(err.to_string().contains("after final answer"));
}

#[test]
fn test_assistant_text_delta_without_started_item_fails() {
    let mut timeline =
        TimelineBuilder::new("request-delta", "turn-delta", "message-1", "session-1");
    let mut state = KernelTimelineState::default();

    let err = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextDelta,
            KernelStreamEventData::AssistantTextDelta(AssistantTextDeltaStreamData {
                item_key: KernelStreamItemKey {
                    round: Some(2),
                    local_id: "missing".to_string(),
                },
                delta: "orphan text".to_string(),
            }),
        ),
    )
    .expect_err("orphan assistant text delta should fail");

    assert!(err
        .to_string()
        .contains("timeline assistant text item not found for delta"));
}

#[test]
fn test_assistant_text_completed_item_key_can_restart() {
    let mut timeline =
        TimelineBuilder::new("request-restart", "turn-restart", "message-1", "session-1");
    let mut state = KernelTimelineState::default();
    let item_key = KernelStreamItemKey {
        round: Some(0),
        local_id: "provider-assistant".to_string(),
    };

    let first_start = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key: item_key.clone(),
                phase: KernelAssistantTextPhase::Commentary,
                round: Some(0),
            }),
        ),
    )
    .expect("first assistant text should start");
    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextCompleted,
            KernelStreamEventData::AssistantTextCompleted(AssistantTextCompletedStreamData {
                item_key: item_key.clone(),
            }),
        ),
    )
    .expect("first assistant text should complete");

    let second_start = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key,
                phase: KernelAssistantTextPhase::Commentary,
                round: Some(0),
            }),
        ),
    )
    .expect("completed provider item key should be reusable");

    let first_item_id = match &first_start[0].payload {
        AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
        other => panic!("expected item started, got {other:?}"),
    };
    let second_item_id = match &second_start[0].payload {
        AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
        other => panic!("expected item started, got {other:?}"),
    };
    assert_ne!(first_item_id, second_item_id);
}

#[test]
fn test_reasoning_item_closes_before_tool_boundary_and_restarts_after_tool() {
    let mut timeline = TimelineBuilder::new(
        "request-reasoning-tool",
        "turn-reasoning-tool",
        "message-1",
        "session-1",
    );
    let mut state = KernelTimelineState::default();

    let first_reasoning_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ThinkingDelta,
            KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                delta: "I need to inspect the workspace.".to_string(),
                source: KernelThinkingSource::Provider,
                stage: KernelThinkingStage::ModelReasoning,
                round: Some(1),
            }),
        ),
    )
    .expect("first reasoning delta should start a reasoning item");
    let first_reasoning_item_id = first_reasoning_events
        .iter()
        .find_map(|event| match &event.payload {
            AgentEventPayload::ItemStarted(payload)
                if payload.item.kind == TalosTimelineItemKind::Reasoning =>
            {
                Some(payload.item.item_id.clone())
            }
            _ => None,
        })
        .expect("reasoning item should be started");

    let tool_start_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolCallStart,
            KernelStreamEventData::ToolCallStart(ToolCallStartStreamData {
                index: 0,
                call_id: Some("call-read".to_string()),
                tool_name: Some("Read".to_string()),
                arguments: json!({"path": "src/lib.rs"}),
                round: 1,
            }),
        ),
    )
    .expect("tool start should close active reasoning first");

    assert!(matches!(
        &tool_start_events[0].payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id == first_reasoning_item_id
                && payload.status == TalosTimelineItemStatus::Completed
    ));
    assert!(matches!(
        &tool_start_events[1].payload,
        AgentEventPayload::ItemStarted(payload)
            if payload.item.kind == TalosTimelineItemKind::ToolCall
                && payload.item.phase == TalosTimelinePhase::Acting
    ));
    assert!(tool_start_events[0].sequence < tool_start_events[1].sequence);
    assert!(state.reasoning_item_id.is_none());

    emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ToolResult,
            KernelStreamEventData::ToolResult(ToolResultStreamData {
                call_id: "call-read".to_string(),
                tool_name: "Read".to_string(),
                content: vec![TalosContentItem::Text {
                    text: "file content".to_string(),
                }],
                is_error: false,
                semantic_events: Vec::new(),
                envelope: None,
                next_turn_failure: None,
            }),
        ),
    )
    .expect("tool result should complete the tool item");

    let second_reasoning_events = emit_kernel_timeline_events(
        &mut timeline,
        &mut state,
        KernelStreamEvent::new(
            KernelStreamEventType::ThinkingDelta,
            KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                delta: "Now I can inspect the result.".to_string(),
                source: KernelThinkingSource::Provider,
                stage: KernelThinkingStage::ModelReasoning,
                round: Some(2),
            }),
        ),
    )
    .expect("later reasoning should start a new reasoning item");
    let second_reasoning_item_id = second_reasoning_events
        .iter()
        .find_map(|event| match &event.payload {
            AgentEventPayload::ItemStarted(payload)
                if payload.item.kind == TalosTimelineItemKind::Reasoning =>
            {
                Some(payload.item.item_id.clone())
            }
            _ => None,
        })
        .expect("new reasoning item should be started");

    assert_ne!(first_reasoning_item_id, second_reasoning_item_id);
}

#[tokio::test]
async fn test_generate_system_prompt() {
    let mock_provider = MockLlmProvider {};
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(mock_provider)),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");

    agent.add_tool(Arc::new(WebSearchTool));
    agent.add_tool(Arc::new(ReadTool));

    let prompt = agent.generate_system_prompt();

    assert!(prompt.contains("Test Agent"));
    assert!(prompt.contains("An agent for testing"));
    assert!(prompt.contains("WebSearch"));
    assert!(prompt.contains("Read"));
}

#[tokio::test]
async fn test_add_tool() {
    let mock_provider = MockLlmProvider {};
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(mock_provider)),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");

    assert_eq!(agent.tool_registry.list_tool_names().len(), 0);

    agent.add_tool(Arc::new(WebSearchTool));

    let tools = agent.tool_registry.list_tool_names();
    assert!(tools.contains(&"WebSearch".to_string()));
}

#[tokio::test]
async fn test_run_uses_agent_runtime_config_and_session_model() {
    let captured_max_tokens = Arc::new(Mutex::new(None));
    let captured_model = Arc::new(Mutex::new(None));
    let provider = Arc::new(CapturingMaxTokensProvider {
        seen_max_tokens: captured_max_tokens.clone(),
        seen_model: captured_model.clone(),
    });
    let mut agent_kernel_config = AgentKernelConfig::default();
    agent_kernel_config.output.max_tokens = 321;
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(provider),
        model_selection("agent-model"),
        Arc::new(ToolRegistry::new()),
        agent_kernel_config,
    )
    .expect("agent harness should build");

    let mut session = build_test_session("session-kernel-override", "workspace-1");
    session.config.agent.kernel.output.max_tokens = 123;
    session.conf.model_selection = Some(model_selection("session-model"));

    let result = agent
        .run(agent_run_params(
            session,
            "turn-kernel-override",
            vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        ))
        .await
        .expect("run should succeed");

    assert_eq!(result.stop_reason, TalosStopReason::EndTurn);
    let seen = captured_max_tokens
        .lock()
        .expect("capture lock poisoned")
        .expect("provider should receive max token setting");
    assert_eq!(seen, 321);
    let seen_model = captured_model
        .lock()
        .expect("model capture lock poisoned")
        .clone()
        .expect("provider should receive model");
    assert_eq!(seen_model, "session-model");
}

#[tokio::test]
async fn test_run_uses_session_provider_for_llm_routing() {
    let seen_provider = Arc::new(Mutex::new(None));
    let seen_model = Arc::new(Mutex::new(None));
    let primary = Arc::new(NamedRecordingProvider {
        provider_name: "primary",
        seen_provider: seen_provider.clone(),
        seen_model: seen_model.clone(),
    });
    let secondary = Arc::new(NamedRecordingProvider {
        provider_name: "secondary",
        seen_provider: seen_provider.clone(),
        seen_model: seen_model.clone(),
    });
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(RecordingLlmProviderRouter { primary, secondary }),
        ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "primary-model".to_string(),
        }),
        Arc::new(ToolRegistry::new()),
        AgentKernelConfig::default(),
    )
    .expect("agent harness should build");

    let mut session = build_test_session("session-provider-routing", "workspace-1");
    session.conf.model_selection = Some(ModelSelection::without_reasoning(ModelRef {
        provider: "deepseek".to_string(),
        id: "secondary-model".to_string(),
    }));

    let result = agent
        .run(agent_run_params(
            session,
            "turn-provider-routing",
            vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        ))
        .await
        .expect("run should succeed");

    assert_eq!(result.stop_reason, TalosStopReason::EndTurn);
    assert_eq!(
        seen_provider
            .lock()
            .expect("provider lock poisoned")
            .as_deref(),
        Some("secondary")
    );
    assert_eq!(
        seen_model.lock().expect("model lock poisoned").as_deref(),
        Some("secondary-model")
    );
}

#[tokio::test]
async fn test_run_generates_title_update_for_first_turn() {
    let agent = build_test_agent();
    let mut session = build_test_session("session-title", "workspace-1");
    session.state.message_count = 0;
    session.title = DEFAULT_SESSION_TITLE.to_string();

    let result = agent
        .run(agent_run_params(
            session,
            "turn-title",
            vec![TalosInputItem::Text {
                text: "Explain the agent harness boundary".to_string(),
            }],
        ))
        .await
        .expect("run should succeed");

    let title = result
        .generated_session_title
        .expect("first turn should produce a generated title");
    assert_eq!(title, "Test response");
}

#[tokio::test]
async fn test_resume_streamed_replays_dynamic_resume_into_model_loop() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let provider = Arc::new(DynamicResumeAgentProvider {
        calls: AtomicUsize::new(0),
    });
    let registry = Arc::new(ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec![
        "echo_dynamic",
    ])));
    registry.register_external_dynamic_tool(RegisteredToolSpec::new(
        ToolDefinition::new(
            "echo_dynamic",
            "External dynamic echo tool",
            Some(json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            })),
        ),
        ToolSource::Dynamic,
        ToolPayloadKind::Function,
        ToolParallelism::ParallelSafe,
        false,
        default_governance_for_source(ToolSource::Dynamic),
    ));
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.prompt.bootstrap.enabled = false;
    kernel_config.workspace_dir = temp_dir.path().display().to_string();
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(provider.clone()),
        model_selection("mock-model"),
        registry,
        kernel_config,
    )
    .expect("agent harness should build");
    let session_id = "session-dynamic-main-resume";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    session.config.agent.kernel.output.max_tokens = 256;
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
    let request_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::DynamicToolRequest {
            payload: session_artifacts::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                arguments: json!({ "message": "ping" }).into(),
                reasoning_continuation: Some(ReasoningContinuation::Text(
                    "dynamic provider continuation".to_string(),
                )),
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    let response_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::DynamicToolResponse {
            payload: session_artifacts::SessionTranscriptDynamicToolResponsePayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                output: "dynamic pong".to_string(),
                success: true,
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    append_transcript_record(&workspace_root, &request_record)
        .await
        .expect("dynamic request record should append");
    append_transcript_record(&workspace_root, &response_record)
        .await
        .expect("dynamic response record should append");

    let mut request = agent_run_params(session, "turn-original", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "tool-item-original".to_string(),
            call_id: "call-dynamic".to_string(),
            tool_name: "echo_dynamic".to_string(),
        }),
    });
    let mut stream = agent
        .resume_streamed(AgentResumeParams::new(request))
        .await
        .expect("resume stream should start");

    let mut saw_tool_result = false;
    let mut saw_final_answer = false;
    while let Some(event) = stream.recv().await {
        match event.payload {
            AgentEventPayload::ItemDelta(payload) => match payload.delta {
                talos_types::talos::TalosTimelineDelta::ToolResult { content, .. } => {
                    saw_tool_result = content.iter().any(|item| {
                        matches!(
                            item,
                            TalosContentItem::Text { text }
                                if text.contains("dynamic pong")
                        ) || matches!(
                            item,
                            TalosContentItem::Json { value }
                                if value.to_string().contains("dynamic pong")
                        )
                    });
                }
                talos_types::talos::TalosTimelineDelta::Text { text }
                    if text.contains("Final answer after dynamic pong.") =>
                {
                    saw_final_answer = true;
                }
                _ => {}
            },
            AgentEventPayload::TurnCompleted(payload) => {
                assert_eq!(payload.stop_reason, TalosStopReason::EndTurn);
                break;
            }
            AgentEventPayload::TurnError(payload) => {
                panic!("resume stream failed: {}", payload.error);
            }
            _ => {}
        }
    }

    assert!(
        saw_tool_result,
        "dynamic resume should emit replayed tool result"
    );
    assert!(
        saw_final_answer,
        "model loop should continue after dynamic replay"
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_session_workspace_resolution_does_not_write_profile_snapshot() {
    let guard = tempfile::tempdir().expect("temp dir should be created");
    let mut session = build_test_session("session-profile-snapshot", "workspace-profile");
    session.config.agent.kernel.workspace_dir = guard.path().display().to_string();
    let profile_path = guard
        .path()
        .join("users")
        .join("talos-test-local-user")
        .join("profile.json");
    std::fs::create_dir_all(profile_path.parent().expect("profile parent should exist"))
        .expect("profile parent should be created");
    std::fs::write(
        &profile_path,
        r#"{"user_id":"talos-test-local-user","username":"Alice","updated_at_ms":1}"#,
    )
    .expect("profile snapshot should be seeded");

    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");

    let profile_content =
        std::fs::read_to_string(&profile_path).expect("profile snapshot should remain readable");
    let profile: serde_json::Value =
        serde_json::from_str(&profile_content).expect("profile snapshot should be json");

    assert_eq!(
        kernel_config.workspace_dir,
        guard
            .path()
            .join("users")
            .join("talos-test-local-user")
            .to_string_lossy()
            .to_string()
    );
    assert_eq!(profile["user_id"], "talos-test-local-user");
    assert_eq!(profile["username"], "Alice");
    assert_eq!(profile["updated_at_ms"], 1);
}

#[test]
fn test_planner_mode_comes_from_kernel_config() {
    let mut agent = build_test_agent();
    agent
        .set_default_planner_mode("built_in")
        .expect("planner mode should parse");
    assert_eq!(
        agent.kernel_config.planner_mode,
        talos_config::PlannerMode::BuiltIn
    );
}

#[tokio::test]
async fn test_submit_dynamic_tool_result_rejects_replay() {
    let (_temp_dir, agent) = build_temp_agent();
    let session_id = "session-dynamic-replay";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.workspace_dir = _temp_dir.path().display().to_string();
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
    let request_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-input".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::DynamicToolRequest {
            payload: session_artifacts::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-1".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({ "name": "Talos" }).into(),
                reasoning_continuation: None,
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    append_transcript_record(&workspace_root, &request_record)
        .await
        .expect("request record should append");

    submit_dynamic_tool_result(
        &agent,
        session.clone(),
        "call-1",
        "skill_greet",
        "hello",
        true,
    )
    .await
    .expect("first submission should succeed");

    let error = submit_dynamic_tool_result(
        &agent,
        session,
        "call-1",
        "skill_greet",
        "hello again",
        true,
    )
    .await
    .expect_err("second submission should be rejected");
    assert_eq!(error.kind, crate::AgentErrorKind::InputRejected);
    assert!(error.to_string().contains("already been resolved"));
}

#[tokio::test]
async fn test_submit_dynamic_tool_result_rejects_timeout() {
    let (_temp_dir, mut agent) = build_temp_agent();
    agent.kernel_config.tools.dynamic.response_timeout_ms = 1;
    let session_id = "session-dynamic-timeout";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.workspace_dir = _temp_dir.path().display().to_string();
    session
        .config
        .agent
        .kernel
        .tools
        .dynamic
        .response_timeout_ms = 1;
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
    let request_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-input".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now() - chrono::Duration::milliseconds(10),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::DynamicToolRequest {
            payload: session_artifacts::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-timeout".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({}).into(),
                reasoning_continuation: None,
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    append_transcript_record(&workspace_root, &request_record)
        .await
        .expect("request record should append");

    let error =
        submit_dynamic_tool_result(&agent, session, "call-timeout", "skill_greet", "late", true)
            .await
            .expect_err("timed out submission should be rejected");
    assert_eq!(error.kind, crate::AgentErrorKind::Timeout);
    assert!(error.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_submit_dynamic_tool_result_rejects_wrong_turn_without_writing_response() {
    let (_temp_dir, agent) = build_temp_agent();
    let session_id = "session-dynamic-wrong-turn";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.workspace_dir = _temp_dir.path().display().to_string();
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
    let request_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-pending".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::DynamicToolRequest {
            payload: session_artifacts::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-wrong-turn".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({}).into(),
                reasoning_continuation: None,
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    append_transcript_record(&workspace_root, &request_record)
        .await
        .expect("request record should append");

    let error = agent
        .submit_input(AgentInputParams {
            session: session.clone(),
            turn_id: "turn-other".to_string(),
            input: AgentInput::DynamicToolResult {
                call_id: "call-wrong-turn".to_string(),
                tool_name: "skill_greet".to_string(),
                output: "wrong turn output".to_string(),
                success: true,
            },
        })
        .await
        .expect_err("wrong turn id should be rejected");

    assert_eq!(error.kind, crate::AgentErrorKind::InputRejected);
    assert!(error.to_string().contains("turn-other"));

    let records = load_transcript_records(
        &workspace_root,
        session_id,
        None,
        session_artifacts::SessionTranscriptBranch::ReactiveLoop,
    )
    .await
    .expect("records should load");
    assert!(records.iter().all(|record| {
        !matches!(
            record.payload,
            session_artifacts::SessionTranscriptPayload::DynamicToolResponse { .. }
        )
    }));
}

#[tokio::test]
async fn test_resume_streamed_rejects_wrong_turn_without_model_call() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let provider = Arc::new(DynamicResumeAgentProvider {
        calls: AtomicUsize::new(0),
    });
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.prompt.bootstrap.enabled = false;
    kernel_config.workspace_dir = temp_dir.path().display().to_string();
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(provider.clone()),
        model_selection("mock-model"),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    )
    .expect("agent harness should build");
    let session_id = "session-dynamic-resume-wrong-turn";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);

    for record in [
        session_artifacts::SessionTranscriptRecord {
            header: session_artifacts::SessionTranscriptHeader {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: "turn-original".to_string(),
                branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: session_artifacts::SessionTranscriptAuthor::System,
            kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
            payload: session_artifacts::SessionTranscriptPayload::DynamicToolRequest {
                payload: session_artifacts::SessionTranscriptDynamicToolRequestPayload {
                    call_id: "call-dynamic".to_string(),
                    tool_name: "echo_dynamic".to_string(),
                    arguments: json!({ "message": "ping" }).into(),
                    reasoning_continuation: None,
                },
            },
            actions: session_artifacts::SessionTranscriptActions::default(),
            partial: false,
        },
        session_artifacts::SessionTranscriptRecord {
            header: session_artifacts::SessionTranscriptHeader {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: "turn-original".to_string(),
                branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: session_artifacts::SessionTranscriptAuthor::System,
            kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
            payload: session_artifacts::SessionTranscriptPayload::DynamicToolResponse {
                payload: session_artifacts::SessionTranscriptDynamicToolResponsePayload {
                    call_id: "call-dynamic".to_string(),
                    tool_name: "echo_dynamic".to_string(),
                    output: "dynamic pong".to_string(),
                    success: true,
                },
            },
            actions: session_artifacts::SessionTranscriptActions::default(),
            partial: false,
        },
    ] {
        append_transcript_record(&workspace_root, &record)
            .await
            .expect("dynamic record should append");
    }

    let mut request = agent_run_params(session, "turn-other", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "tool-item-original".to_string(),
            call_id: "call-dynamic".to_string(),
            tool_name: "echo_dynamic".to_string(),
        }),
    });
    let mut stream = agent
        .resume_streamed(AgentResumeParams::new(request))
        .await
        .expect("stream should start and report runtime error");

    let event = stream
        .recv()
        .await
        .expect("wrong-turn resume should emit an error event");
    match event.payload {
        AgentEventPayload::TurnError(payload) => {
            assert!(payload.error.contains("turn-other"));
        }
        other => panic!("expected TurnError, got {other:?}"),
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_resume_streamed_rejects_missing_tool_call_without_model_call() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let provider = Arc::new(DynamicResumeAgentProvider {
        calls: AtomicUsize::new(0),
    });
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.prompt.bootstrap.enabled = false;
    kernel_config.workspace_dir = temp_dir.path().display().to_string();
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(provider.clone()),
        model_selection("mock-model"),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    )
    .expect("agent harness should build");
    let mut session = build_test_session("session-missing-tool-call", "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();

    let mut request = agent_run_params(session, "turn-missing-tool-call", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext { tool_call: None });
    let mut stream = agent
        .resume_streamed(AgentResumeParams::new(request))
        .await
        .expect("stream should start and report invalid resume context");

    let event = stream
        .recv()
        .await
        .expect("missing tool call should emit an error event");
    match event.payload {
        AgentEventPayload::TurnError(payload) => {
            assert!(payload.error.contains("missing tool call metadata"));
        }
        other => panic!("expected TurnError, got {other:?}"),
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_resume_streamed_rejects_missing_resume_context() {
    let agent = build_test_agent();
    let session = build_test_session("session-no-resume-context", "workspace-1");
    let request = agent_run_params(session, "turn-no-resume-context", Vec::new());

    let error = match agent.resume_streamed(AgentResumeParams::new(request)).await {
        Ok(_) => panic!("resume_streamed should reject a missing resume context"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("resume requires"));
    assert!(error.to_string().contains("turn-no-resume-context"));
}

#[tokio::test]
async fn test_run_rejects_resume_context() {
    let agent = build_test_agent();
    let session = build_test_session("session-run-with-resume-context", "workspace-1");
    let mut request = agent_run_params(session, "turn-run-with-resume-context", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "tool-item-original".to_string(),
            call_id: "call-dynamic".to_string(),
            tool_name: "echo_dynamic".to_string(),
        }),
    });

    let error = match agent.run(request).await {
        Ok(_) => panic!("run should reject continuation context"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("use resume API"));
    assert!(error.to_string().contains("turn-run-with-resume-context"));
}

#[tokio::test]
async fn test_run_streamed_rejects_resume_context() {
    let agent = build_test_agent();
    let session = build_test_session("session-stream-with-resume-context", "workspace-1");
    let mut request = agent_run_params(session, "turn-stream-with-resume-context", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "tool-item-original".to_string(),
            call_id: "call-dynamic".to_string(),
            tool_name: "echo_dynamic".to_string(),
        }),
    });

    let error = match agent.run_streamed(request).await {
        Ok(_) => panic!("run_streamed should reject continuation context"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("use resume API"));
    assert!(error
        .to_string()
        .contains("turn-stream-with-resume-context"));
}

#[tokio::test]
async fn test_resume_streamed_rejects_missing_request_without_model_call() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let provider = Arc::new(DynamicResumeAgentProvider {
        calls: AtomicUsize::new(0),
    });
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.prompt.bootstrap.enabled = false;
    kernel_config.workspace_dir = temp_dir.path().display().to_string();
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(provider.clone()),
        model_selection("mock-model"),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    )
    .expect("agent harness should build");
    let session_id = "session-missing-dynamic-request";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::PathBuf::from(&kernel_config.workspace_dir);
    let response_record = session_artifacts::SessionTranscriptRecord {
        header: session_artifacts::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: session_artifacts::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: session_artifacts::SessionTranscriptAuthor::System,
        kind: session_artifacts::SessionTranscriptEventKind::SystemMessage,
        payload: session_artifacts::SessionTranscriptPayload::DynamicToolResponse {
            payload: session_artifacts::SessionTranscriptDynamicToolResponsePayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                output: "dynamic pong".to_string(),
                success: true,
            },
        },
        actions: session_artifacts::SessionTranscriptActions::default(),
        partial: false,
    };
    append_transcript_record(&workspace_root, &response_record)
        .await
        .expect("dynamic response record should append");

    let mut request = agent_run_params(session, "turn-original", Vec::new());
    request.resume_context = Some(AgentDispatchResumeContext {
        tool_call: Some(AgentDispatchResumeToolCall {
            item_id: "tool-item-original".to_string(),
            call_id: "call-dynamic".to_string(),
            tool_name: "echo_dynamic".to_string(),
        }),
    });
    let mut stream = agent
        .resume_streamed(AgentResumeParams::new(request))
        .await
        .expect("stream should start and report missing dynamic request");

    let event = stream
        .recv()
        .await
        .expect("missing request should emit an error event");
    match event.payload {
        AgentEventPayload::TurnError(payload) => {
            assert!(payload.error.contains("no pending request"));
        }
        other => panic!("expected TurnError, got {other:?}"),
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_interrupt_aborts_only_current_session_turn() {
    let started = Arc::new(Notify::new());
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");
    let session = build_test_session("session-cancel", "workspace-a");

    let mut first_stream = agent
        .run_streamed(agent_run_params(
            session.clone(),
            "turn-cancel",
            vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
        ))
        .await
        .expect("first stream should start");
    started.notified().await;
    let wrong_turn_error = agent
        .interrupt(AgentInterruptParams {
            session_id: session.context.session_id.clone(),
            turn_id: "turn-other".to_string(),
        })
        .await
        .expect_err("wrong turn id should not interrupt current run");
    assert_eq!(wrong_turn_error.kind, crate::AgentErrorKind::RunNotFound);
    agent
        .interrupt(AgentInterruptParams {
            session_id: session.context.session_id.clone(),
            turn_id: "turn-cancel".to_string(),
        })
        .await
        .expect("interrupt should succeed");

    let mut first_stop_reason = None;
    while let Some(event) = first_stream.recv().await {
        if let AgentEventPayload::TurnCompleted(payload) = event.payload {
            first_stop_reason = Some(payload.stop_reason);
            break;
        }
    }
    assert_eq!(first_stop_reason, Some(TalosStopReason::Cancelled));

    let mut second_stream = agent
        .run_streamed(agent_run_params(
            session,
            "turn-second",
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        ))
        .await
        .expect("second stream should start");
    let mut second_stop_reason = None;
    while let Some(event) = second_stream.recv().await {
        if let AgentEventPayload::TurnCompleted(payload) = event.payload {
            second_stop_reason = Some(payload.stop_reason);
            break;
        }
    }
    assert_eq!(second_stop_reason, Some(TalosStopReason::EndTurn));
}

#[tokio::test]
async fn test_interrupt_reports_missing_runtime_state() {
    let agent = build_test_agent();

    let error = interrupt_session(&agent, "missing-session")
        .await
        .expect_err("missing active turn should be reported");

    assert_eq!(error.kind, crate::AgentErrorKind::RunNotFound);
}

#[tokio::test]
async fn test_interrupt_rejects_reserved_turn_without_runtime_handle() {
    let agent = build_test_agent();
    agent
        .try_reserve_active_turn("reserved-session")
        .expect("test should reserve active slot");

    let error = interrupt_session(&agent, "reserved-session")
        .await
        .expect_err("reserved turn cannot report a successful interrupt");

    assert_eq!(error.kind, crate::AgentErrorKind::RunNotFound);
}
#[tokio::test]
async fn clear_session_advances_memory_cursor_and_preserves_turn_sequence() {
    let (workspace, agent) = build_temp_agent();
    let session_id = "clear-session";
    let mut session = build_test_session(session_id, "workspace-a");
    session.config.agent.kernel.workspace_dir = workspace.path().display().to_string();
    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("session workspace should resolve");
    let workspace_root = std::path::Path::new(&kernel_config.workspace_dir);
    let artifact_store = FileSessionArtifactStore::default();
    artifact_store
        .append_turn(
            workspace_root,
            &session_artifacts::TalosTurn {
                turn_id: "turn-1".to_string(),
                message_id: "message-1".to_string(),
                session_id: session_id.to_string(),
                seq: 1,
                timestamp: Utc::now(),
                items: vec![talos_types::talos::RunItem::UserInput {
                    text: "discarded by clear".to_string(),
                }],
                usage: TokenUsage::default(),
                stop_reason: TalosStopReason::EndTurn,
            },
        )
        .await
        .expect("append turn");

    agent.clear_session(session).await.expect("clear session");

    assert_eq!(
        agent
            .session_store
            .get_session_record(session_id)
            .await
            .expect("load session record")
            .expect("session record")
            .memory_processed_until_seq,
        1
    );
    let restarted_store = FileSessionArtifactStore::default();
    assert_eq!(
        restarted_store
            .load_turns(workspace_root, session_id)
            .await
            .expect("load turns after clear"),
        Vec::new()
    );
    assert_eq!(
        restarted_store
            .next_turn_seq(workspace_root, session_id)
            .await
            .expect("next turn sequence"),
        2
    );
}
#[tokio::test]
async fn test_run_streamed_rejects_second_active_turn_for_same_session() {
    let started = Arc::new(Notify::new());
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");
    let session = build_test_session("session-busy", "workspace-a");

    let mut first_stream = agent
        .run_streamed(agent_run_params(
            session.clone(),
            "turn-active",
            vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
        ))
        .await
        .expect("first stream should start");
    started.notified().await;

    let second_error = agent
        .run_streamed(agent_run_params(
            session.clone(),
            "turn-rejected",
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        ))
        .await
        .expect_err("second stream should be rejected");
    assert_eq!(second_error.kind, crate::AgentErrorKind::ActiveTurnRunning);

    interrupt_session(&agent, &session.context.session_id)
        .await
        .expect("interrupt should succeed");

    let mut first_stop_reason = None;
    while let Some(event) = first_stream.recv().await {
        if let AgentEventPayload::TurnCompleted(payload) = event.payload {
            first_stop_reason = Some(payload.stop_reason);
            break;
        }
    }
    assert_eq!(first_stop_reason, Some(TalosStopReason::Cancelled));
}

#[tokio::test]
async fn test_run_streamed_allows_different_sessions_concurrently() {
    let started = Arc::new(Notify::new());
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");
    let first_session = build_test_session("session-active", "workspace-a");
    let second_session = build_test_session("session-independent", "workspace-a");

    let mut first_stream = agent
        .run_streamed(agent_run_params(
            first_session.clone(),
            "turn-active",
            vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
        ))
        .await
        .expect("first stream should start");
    started.notified().await;

    let mut second_stream = agent
        .run_streamed(agent_run_params(
            second_session,
            "turn-independent",
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        ))
        .await
        .expect("different session should start");
    let mut second_stop_reason = None;
    while let Some(event) = second_stream.recv().await {
        if let AgentEventPayload::TurnCompleted(payload) = event.payload {
            second_stop_reason = Some(payload.stop_reason);
            break;
        }
    }
    assert_eq!(second_stop_reason, Some(TalosStopReason::EndTurn));

    interrupt_session(&agent, &first_session.context.session_id)
        .await
        .expect("first session interrupt should succeed");
    while let Some(event) = first_stream.recv().await {
        if matches!(event.payload, AgentEventPayload::TurnCompleted(_)) {
            break;
        }
    }
}

#[tokio::test]
async fn test_run_streamed_emits_user_input_and_single_final_answer() {
    let agent = build_test_agent();
    let session = build_test_session("session-timeline", "workspace-a");

    let mut stream = agent
        .run_streamed(agent_run_params(
            session,
            "turn-timeline",
            vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        ))
        .await
        .expect("stream should start");

    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        let terminal = matches!(
            event.payload,
            AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnError(_)
        );
        events.push(event);
        if terminal {
            break;
        }
    }

    let mut user_input_item_id = None;
    let mut final_answer_item_id = None;
    let mut unknown_answer_item_id = None;
    let mut commentary_count = 0;
    for event in &events {
        match &event.payload {
            AgentEventPayload::ItemStarted(payload)
                if payload.item.kind == TalosTimelineItemKind::UserInput =>
            {
                assert_eq!(payload.item.phase, TalosTimelinePhase::Input);
                user_input_item_id = Some(payload.item.item_id.clone());
            }
            AgentEventPayload::ItemStarted(payload)
                if payload.item.phase == TalosTimelinePhase::FinalAnswer =>
            {
                final_answer_item_id = Some(payload.item.item_id.clone());
            }
            AgentEventPayload::ItemStarted(payload)
                if payload.item.phase == TalosTimelinePhase::Unknown =>
            {
                unknown_answer_item_id = Some(payload.item.item_id.clone());
            }
            AgentEventPayload::AssistantTextCommitted(payload)
                if payload.phase == TalosTimelinePhase::FinalAnswer =>
            {
                final_answer_item_id = Some(payload.item_id.clone());
            }
            AgentEventPayload::ItemStarted(payload)
                if payload.item.phase == TalosTimelinePhase::Commentary =>
            {
                commentary_count += 1;
            }
            _ => {}
        }
    }

    let user_input_item_id = user_input_item_id.expect("user input item should start");
    let final_answer_item_id = final_answer_item_id.expect("final answer item should start");
    assert_eq!(
        unknown_answer_item_id.as_deref(),
        Some(final_answer_item_id.as_str())
    );
    assert_eq!(commentary_count, 0);
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id == user_input_item_id
                && payload.status == TalosTimelineItemStatus::Completed
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemDelta(payload)
            if payload.item_id == final_answer_item_id
                && matches!(payload.delta, TalosTimelineDelta::Text { .. })
    )));
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemCompleted(payload)
            if payload.item_id == final_answer_item_id
                && payload.status == TalosTimelineItemStatus::Completed
    )));
}

#[tokio::test]
async fn test_run_rejects_second_active_turn_for_same_session() {
    let started = Arc::new(Notify::new());
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        static_router(Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        })),
        model_selection("gpt-4o"),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
    .expect("agent harness should build");
    let agent = Arc::new(agent);
    let session = build_test_session("session-run-queue", "workspace-a");

    let first_agent = agent.clone();
    let first_session = session.clone();
    let first_run = tokio::spawn(async move {
        first_agent
            .run(agent_run_params(
                first_session,
                "turn-active",
                vec![TalosInputItem::Text {
                    text: "first".to_string(),
                }],
            ))
            .await
    });
    started.notified().await;

    let second_error = agent
        .run(agent_run_params(
            session.clone(),
            "turn-run-rejected",
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        ))
        .await
        .expect_err("second unary turn should be rejected");
    assert_eq!(second_error.kind, crate::AgentErrorKind::ActiveTurnRunning);

    interrupt_session(&agent, &session.context.session_id)
        .await
        .expect("interrupt should succeed");

    let first_result = first_run
        .await
        .expect("first join should succeed")
        .expect("first execution should complete");
    assert_eq!(first_result.stop_reason, TalosStopReason::Cancelled);
}

struct MockLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        let content = if request.messages.iter().any(|message| {
            message
                .content
                .contains("Extract durable memory from this conversation as JSON.")
        }) {
            "{\"title\":\"Durable Memory\",\"facts\":[\"User prefers tea over coffee\"]}"
                .to_string()
        } else {
            "Test response".to_string()
        };
        Ok(ChatResponse {
            content,
            reasoning: ReasoningOutput::default(),
            model: "mock-model".to_string(),
            usage: Usage { ..Usage::default() },
            finish_reason: None,
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        drop(tx);
        Ok(ChatStream { rx })
    }

    async fn stream_model_blocks(
        &self,
        _request: ChatRequest,
    ) -> talos_error::TalosResult<ModelBlockStream> {
        Ok(single_text_block_stream("Test response"))
    }
}
