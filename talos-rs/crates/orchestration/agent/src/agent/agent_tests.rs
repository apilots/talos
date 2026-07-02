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
    DynamicToolRequestStreamData, ExecutionSummaryStreamData, KernelAssistantTextPhase,
    KernelStreamEvent, KernelStreamEventData, KernelStreamEventType, KernelStreamItemKey,
    KernelThinkingSource, KernelThinkingStage, ThinkingDeltaStreamData, ToolArgsDeltaStreamData,
    ToolCallStartStreamData, ToolResultStreamData, ToolStreamKey,
};
use talos_config::AgentKernelConfig;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockCompleted, ModelBlockDelta,
    ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent, Role, ToolDefinition, Usage,
};
use talos_memory::{Memory, TalosTurn};
use talos_tools::{
    default_governance_for_source, ReadTool, RegisteredToolSpec, ToolParallelism, ToolPayloadKind,
    ToolRegistry, ToolSource, WebSearchTool,
};
use talos_tools_policy::ToolPolicy;
use talos_types::agent_dispatch::{AgentDispatchResumeContext, AgentDispatchResumeToolCall};
use talos_types::talos::{
    AgentEventPayload, RunItem, StopReason as TalosStopReason, TalosContentItem, TalosInputItem,
    TalosProvider, TalosSourceType, TalosTimelineDelta, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TokenUsage,
};
use talos_types::{SessionConf, SessionContext, SessionState, TalosSession, DEFAULT_SESSION_TITLE};
use tempfile::TempDir;
use tokio::sync::Notify;
use tokio::time::{timeout, Duration};

fn build_test_agent() -> AgentHarnessImpl {
    AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(MockLlmProvider {}),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
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
            self.started.notify_waiters();
            pending::<talos_error::TalosResult<ChatResponse>>().await?;
            unreachable!("pending future should not resolve");
        }

        Ok(ChatResponse {
            content: "provider ok".to_string(),
            reasoning: Vec::new(),
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
            self.started.notify_waiters();
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
            reasoning: Vec::new(),
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
            reasoning: Vec::new(),
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
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.workspace_dir = temp_dir.path().display().to_string();
    kernel_config.memory.compaction_threshold_bytes = usize::MAX;
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(MockLlmProvider {}),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    );
    (temp_dir, agent)
}

fn build_test_session(session_id: &str, workspace_id: &str) -> TalosSession {
    TalosSession {
        title: "Untitled Session".to_string(),
        user_id: "talos-test-local-user".to_string(),
        conf: SessionConf::default(),
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
        config: talos_config::Config::default(),
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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(mock_provider),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(mock_provider),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    assert_eq!(agent.tool_registry.list_tool_names().len(), 0);

    agent.add_tool(Arc::new(WebSearchTool));

    let tools = agent.tool_registry.list_tool_names();
    assert!(tools.contains(&"WebSearch".to_string()));
}

#[tokio::test]
async fn test_run_uses_agent_runtime_config_and_model_override() {
    let captured_max_tokens = Arc::new(Mutex::new(None));
    let captured_model = Arc::new(Mutex::new(None));
    let provider = Arc::new(CapturingMaxTokensProvider {
        seen_max_tokens: captured_max_tokens.clone(),
        seen_model: captured_model.clone(),
    });
    let mut agent_kernel_config = AgentKernelConfig::default();
    agent_kernel_config.output.max_tokens = 321;
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        provider,
        "agent-model".to_string(),
        Arc::new(ToolRegistry::new()),
        agent_kernel_config,
    );

    let mut session = build_test_session("session-kernel-override", "workspace-1");
    session.config.agent.kernel.output.max_tokens = 123;
    session.config.llm.model = "session-model".to_string();
    session.config.agent.model = "session-model".to_string();

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
    assert_eq!(seen_model, "agent-model");
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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        provider.clone(),
        "mock-model".to_string(),
        registry,
        kernel_config,
    );
    let session_id = "session-dynamic-main-resume";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    session.config.agent.kernel.output.max_tokens = 256;
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");
    let request_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
            payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                arguments: json!({ "message": "ping" }).into(),
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    let response_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolResponse {
            payload: talos_memory::SessionTranscriptDynamicToolResponsePayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                output: "dynamic pong".to_string(),
                success: true,
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    memory_store
        .append_transcript_record(&request_record)
        .await
        .expect("dynamic request record should append");
    memory_store
        .append_transcript_record(&response_record)
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
                talos_types::talos::TalosTimelineDelta::Text { text } => {
                    if text.contains("Final answer after dynamic pong.") {
                        saw_final_answer = true;
                    }
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
async fn test_session_memory_store_does_not_write_profile_snapshot() {
    let (guard, agent) = build_temp_agent();
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

    let (kernel_config, _memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");

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

#[tokio::test]
async fn test_close_session_flushes_workspace_memory() {
    let (_guard, agent) = build_temp_agent();
    let session_id = "session-close-flush";
    let mut session = build_test_session(session_id, "workspace-a");
    session.config.agent.kernel.workspace_dir = _guard.path().display().to_string();
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");
    let timestamp = Utc::now();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: session_id.to_string(),
        seq: 1,
        timestamp,
        items: vec![
            RunItem::UserInput {
                text: "Remember that I prefer tea over coffee.".to_string(),
            },
            RunItem::OutputText {
                text: "Noted. You prefer tea over coffee.".to_string(),
            },
        ],
        usage: TokenUsage::default(),
        stop_reason: TalosStopReason::EndTurn,
    };

    memory_store
        .append_turn(&turn)
        .await
        .expect("session turn should be persisted");

    agent
        .close_session(session)
        .await
        .expect("session close should enqueue workspace memory flush");
    agent
        .drain_pending_memory_flushes(Duration::from_secs(5))
        .await
        .expect("memory flush should complete during drain");

    let file_path = _guard
        .path()
        .join("users")
        .join("talos-test-local-user")
        .join("memory")
        .join(format!("{}.md", Utc::now().format("%Y-%m-%d")));
    let mut content = None;
    for _ in 0..20 {
        match tokio::fs::read_to_string(&file_path).await {
            Ok(value) => {
                content = Some(value);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(25)).await,
        }
    }
    let content = content.expect("workspace daily memory file should exist");
    assert!(content.contains("## ["));
    assert!(content.contains("session:"));
    assert!(content.contains("Durable Memory"));
    assert!(!content.trim().is_empty());
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
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");
    let request_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-input".to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
            payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-1".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({ "name": "Talos" }).into(),
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    memory_store
        .append_transcript_record(&request_record)
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
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");
    let request_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-input".to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now() - chrono::Duration::milliseconds(10),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
            payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-timeout".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({}).into(),
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    memory_store
        .append_transcript_record(&request_record)
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
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");
    let request_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-pending".to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
            payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-wrong-turn".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({}).into(),
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    memory_store
        .append_transcript_record(&request_record)
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

    let records = memory_store
        .load_transcript_records(
            session_id,
            None,
            talos_memory::SessionTranscriptBranch::ReactiveLoop,
        )
        .await
        .expect("records should load");
    assert!(records.iter().all(|record| {
        !matches!(
            record.payload,
            talos_memory::SessionTranscriptPayload::DynamicToolResponse { .. }
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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        provider.clone(),
        "mock-model".to_string(),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    );
    let session_id = "session-dynamic-resume-wrong-turn";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");

    for record in [
        talos_memory::SessionTranscriptRecord {
            header: talos_memory::SessionTranscriptHeader {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: "turn-original".to_string(),
                branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: talos_memory::SessionTranscriptAuthor::System,
            kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
            payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
                payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                    call_id: "call-dynamic".to_string(),
                    tool_name: "echo_dynamic".to_string(),
                    arguments: json!({ "message": "ping" }).into(),
                },
            },
            actions: talos_memory::SessionTranscriptActions::default(),
            partial: false,
        },
        talos_memory::SessionTranscriptRecord {
            header: talos_memory::SessionTranscriptHeader {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.to_string(),
                invocation_id: "turn-original".to_string(),
                branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
                timestamp: Utc::now(),
            },
            author: talos_memory::SessionTranscriptAuthor::System,
            kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
            payload: talos_memory::SessionTranscriptPayload::DynamicToolResponse {
                payload: talos_memory::SessionTranscriptDynamicToolResponsePayload {
                    call_id: "call-dynamic".to_string(),
                    tool_name: "echo_dynamic".to_string(),
                    output: "dynamic pong".to_string(),
                    success: true,
                },
            },
            actions: talos_memory::SessionTranscriptActions::default(),
            partial: false,
        },
    ] {
        memory_store
            .append_transcript_record(&record)
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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        provider.clone(),
        "mock-model".to_string(),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    );
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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        provider.clone(),
        "mock-model".to_string(),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    );
    let session_id = "session-missing-dynamic-request";
    let mut session = build_test_session(session_id, "workspace-1");
    session.config.agent.kernel.prompt.bootstrap.enabled = false;
    session.config.agent.kernel.workspace_dir = temp_dir.path().display().to_string();
    let (_kernel_config, memory_store) = agent
        .memory_store_for_session(&session)
        .await
        .expect("session memory store should build");
    let response_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: "turn-original".to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolResponse {
            payload: talos_memory::SessionTranscriptDynamicToolResponsePayload {
                call_id: "call-dynamic".to_string(),
                tool_name: "echo_dynamic".to_string(),
                output: "dynamic pong".to_string(),
                success: true,
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    memory_store
        .append_transcript_record(&response_record)
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
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        }),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
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
async fn test_run_streamed_queues_second_active_turn_for_same_session() {
    let started = Arc::new(Notify::new());
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        }),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
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

    let mut second_stream = agent
        .run_streamed(agent_run_params(
            session.clone(),
            "turn-queued",
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        ))
        .await
        .expect("second stream should still return a receiver");

    assert!(
        timeout(Duration::from_millis(100), second_stream.recv())
            .await
            .is_err(),
        "queued stream should wait until the active turn finishes"
    );

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
async fn test_run_queues_second_active_turn_for_same_session() {
    let started = Arc::new(Notify::new());
    let agent = AgentHarnessImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(SequencedProvider {
            started: started.clone(),
            calls: AtomicUsize::new(0),
        }),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
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

    let second_agent = agent.clone();
    let second_session = session.clone();
    let mut second_run = Box::pin(tokio::spawn(async move {
        second_agent
            .run(agent_run_params(
                second_session,
                "turn-run-queued",
                vec![TalosInputItem::Text {
                    text: "second".to_string(),
                }],
            ))
            .await
    }));

    assert!(
        timeout(Duration::from_millis(100), &mut second_run)
            .await
            .is_err(),
        "queued unary turn should not finish while the active turn is still running"
    );

    interrupt_session(&agent, &session.context.session_id)
        .await
        .expect("interrupt should succeed");

    let first_result = first_run
        .await
        .expect("first join should succeed")
        .expect("first execution should complete");
    assert_eq!(first_result.stop_reason, TalosStopReason::Cancelled);

    let second_result = second_run
        .await
        .expect("second join should succeed")
        .expect("queued execution should complete");
    assert_eq!(second_result.stop_reason, TalosStopReason::EndTurn);
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
            reasoning: Vec::new(),
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
