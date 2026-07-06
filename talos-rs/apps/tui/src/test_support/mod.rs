pub(crate) use super::*;
use crate::utils::char_display_width;
use async_trait::async_trait;
pub(crate) use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
pub(crate) use serde_json::json;
pub(crate) use std::path::PathBuf;
use std::sync::Mutex;
use talos_error::TalosError;
pub(crate) use talos_sdk::{
    AgentEvent, AgentEventPayload, AgentEventType, AuthSessionReadResponse, AuthSessionSnapshot,
    AuthSessionUserSnapshot, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, SessionHistorySnapshot, SkillSuggestion, StartTurnRequest,
    StopReason, TalosContentItem, TalosInputItem, TalosTimelineContent, TalosTimelineDelta,
    TalosTimelineItem, TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase,
    TimelineActiveTurnSnapshot, TimelineActiveTurnStatus, TimelineCursorStatus,
    TimelineEventEnvelope, TimelineRecoveryResponse, TimelineReplay, TurnExecutionMetrics,
    UserRole, WorkspaceInfoResponse,
};
pub(crate) use talos_types::TimelineRecoverySnapshot;

#[derive(Default)]
pub(crate) struct MockGatewayState {
    pub(crate) models: Vec<ModelInfo>,
    pub(crate) session_id: String,
    pub(crate) current_model: String,
    pub(crate) current_project_root: Option<PathBuf>,
    pub(crate) auth_email: Option<String>,
    pub(crate) workspace_root: PathBuf,
    pub(crate) updated_model: Option<String>,
    pub(crate) stream_counter: u64,
    pub(crate) list_models_failures_remaining: u8,
    pub(crate) connect_delay_ms: u64,
    pub(crate) stream_failures_remaining: u8,
    pub(crate) close_session_delay_ms: u64,
    pub(crate) close_session_failures_remaining: u8,
    pub(crate) shutdown_calls: usize,
    pub(crate) cancelled_sessions: Vec<String>,
    pub(crate) session_usage: talos_types::talos::TokenUsage,
    pub(crate) compact_calls: usize,
    pub(crate) clear_calls: usize,
    pub(crate) reconnect_calls: usize,
    pub(crate) reconnect_since_event_ids: Vec<Option<String>>,
    pub(crate) reconnect_events: Vec<TimelineEventEnvelope>,
    pub(crate) sent_requests: Vec<StartTurnRequest>,
}

pub(crate) struct MockGatewayClient {
    pub(crate) state: Arc<Mutex<MockGatewayState>>,
}

pub(crate) fn mock_model_info(id: &str, context_window: u32) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        name: id.to_string(),
        provider: "mock".to_string(),
        context_window,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: vec![talos_types::ModelInputKind::Text],
        cost: talos_types::ModelCostConfig::default(),
        compat: talos_types::ModelCompatConfig::default(),
    }
}

impl MockGatewayState {
    fn session_info(&self, message_count: u64) -> SessionInfo {
        SessionInfo {
            id: self.session_id.clone(),
            title: "Untitled Session".to_string(),
            user_id: "test-user".to_string(),
            source_type: talos_types::talos::TalosSourceType::Http,
            provider: talos_types::talos::TalosProvider::Sdk,
            model_ref: talos_sdk::ModelRef {
                provider: "mock".to_string(),
                id: self.current_model.clone(),
            },
            created_at: 1,
            last_activity: 2,
            message_count,
            usage: self.session_usage.clone(),
            project_id: None,
            current_directory: self.current_project_root.clone(),
        }
    }
}

pub(crate) fn started_event(message_id: impl Into<String>, sequence: u64) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        "sess-test".to_string(),
        sequence,
        AgentEventType::TurnStarted,
        AgentEventPayload::TurnStarted(talos_sdk::TalosTurnStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            started_at_ms: sequence,
        }),
    )
}

pub(crate) fn output_item_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
) -> TimelineEventEnvelope {
    let item_id = "final-answer".to_string();
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id,
                parent_item_id: None,
                kind: TalosTimelineItemKind::AssistantText,
                phase: TalosTimelinePhase::FinalAnswer,
                status: TalosTimelineItemStatus::Running,
                title: None,
                started_at_ms: sequence,
                completed_at_ms: None,
                content: TalosTimelineContent::Text {
                    text: String::new(),
                },
            },
        }),
    )
}

pub(crate) fn output_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemDelta,
        AgentEventPayload::ItemDelta(talos_sdk::TalosItemDeltaEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: "final-answer".to_string(),
            delta: TalosTimelineDelta::Text { text: delta.into() },
        }),
    )
}

pub(crate) fn output_completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    text: impl Into<String>,
) -> TimelineEventEnvelope {
    output_completed_event_for_item(message_id, session_id, sequence, "final-answer", text)
}

pub(crate) fn output_completed_event_for_item(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    item_id: impl Into<String>,
    text: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemCompleted,
        AgentEventPayload::ItemCompleted(talos_sdk::TalosItemCompletedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: item_id.into(),
            status: TalosTimelineItemStatus::Completed,
            completed_at_ms: sequence,
            final_content: Some(TalosTimelineContent::Text { text: text.into() }),
        }),
    )
}

pub(crate) fn commentary_item_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id: "commentary".to_string(),
                parent_item_id: None,
                kind: TalosTimelineItemKind::AssistantText,
                phase: TalosTimelinePhase::Commentary,
                status: TalosTimelineItemStatus::Running,
                title: None,
                started_at_ms: sequence,
                completed_at_ms: None,
                content: TalosTimelineContent::Text {
                    text: String::new(),
                },
            },
        }),
    )
}

pub(crate) fn commentary_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemDelta,
        AgentEventPayload::ItemDelta(talos_sdk::TalosItemDeltaEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: "commentary".to_string(),
            delta: TalosTimelineDelta::Text { text: delta.into() },
        }),
    )
}

pub(crate) fn unknown_item_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
) -> TimelineEventEnvelope {
    text_item_started_event(
        message_id,
        session_id,
        sequence,
        "unknown-answer",
        TalosTimelinePhase::Unknown,
        String::new(),
    )
}

pub(crate) fn text_item_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    item_id: impl Into<String>,
    phase: TalosTimelinePhase,
    text: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id: item_id.into(),
                parent_item_id: None,
                kind: TalosTimelineItemKind::AssistantText,
                phase,
                status: TalosTimelineItemStatus::Running,
                title: None,
                started_at_ms: sequence,
                completed_at_ms: None,
                content: TalosTimelineContent::Text { text: text.into() },
            },
        }),
    )
}

pub(crate) fn unknown_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEventEnvelope {
    text_delta_event(message_id, session_id, sequence, "unknown-answer", delta)
}

pub(crate) fn text_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    item_id: impl Into<String>,
    delta: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemDelta,
        AgentEventPayload::ItemDelta(talos_sdk::TalosItemDeltaEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: item_id.into(),
            delta: TalosTimelineDelta::Text { text: delta.into() },
        }),
    )
}

pub(crate) fn assistant_text_committed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    phase: TalosTimelinePhase,
) -> TimelineEventEnvelope {
    assistant_text_committed_event_for_item(
        message_id,
        session_id,
        sequence,
        "unknown-answer",
        phase,
    )
}

pub(crate) fn assistant_text_committed_event_for_item(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    item_id: impl Into<String>,
    phase: TalosTimelinePhase,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::AssistantTextCommitted,
        AgentEventPayload::AssistantTextCommitted(talos_sdk::TalosAssistantTextCommittedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: item_id.into(),
            phase,
        }),
    )
}

pub(crate) fn reasoning_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    delta: impl Into<String>,
) -> TimelineEventEnvelope {
    let item_id = "reasoning".to_string();
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id,
                parent_item_id: None,
                kind: TalosTimelineItemKind::Reasoning,
                phase: TalosTimelinePhase::Thinking,
                status: TalosTimelineItemStatus::Running,
                title: None,
                started_at_ms: sequence,
                completed_at_ms: None,
                content: TalosTimelineContent::Text { text: delta.into() },
            },
        }),
    )
}

pub(crate) fn completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    usage: TokenUsage,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::TurnCompleted,
        AgentEventPayload::TurnCompleted(talos_sdk::TalosTurnCompletedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            completed_at_ms: sequence,
            stop_reason: StopReason::EndTurn,
            usage,
            metrics: None,
        }),
    )
}

pub(crate) fn completed_event_with_metrics(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    usage: TokenUsage,
    metrics: TurnExecutionMetrics,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::TurnCompleted,
        AgentEventPayload::TurnCompleted(talos_sdk::TalosTurnCompletedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            completed_at_ms: sequence,
            stop_reason: StopReason::EndTurn,
            usage,
            metrics: Some(metrics),
        }),
    )
}

pub(crate) fn turn_metrics_event_with_metrics(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    metrics: TurnExecutionMetrics,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::TurnMetrics,
        AgentEventPayload::TurnMetrics(talos_sdk::TalosTurnMetricsEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            metrics,
        }),
    )
}

pub(crate) fn execution_summary_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    summary: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id: format!("summary-{sequence}"),
                parent_item_id: None,
                kind: TalosTimelineItemKind::ExecutionSummary,
                phase: TalosTimelinePhase::System,
                status: TalosTimelineItemStatus::Completed,
                title: None,
                started_at_ms: sequence,
                completed_at_ms: Some(sequence),
                content: TalosTimelineContent::ExecutionSummary {
                    summary: summary.into(),
                },
            },
        }),
    )
}

pub(crate) fn context_compaction_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    stage: ContextCompactionStage,
    message: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id: "context-compaction".to_string(),
                parent_item_id: None,
                kind: TalosTimelineItemKind::ContextCompaction,
                phase: TalosTimelinePhase::System,
                status: TalosTimelineItemStatus::Running,
                title: None,
                started_at_ms: sequence,
                completed_at_ms: None,
                content: TalosTimelineContent::ContextCompaction {
                    trigger: ContextCompactionTrigger::ThresholdExceeded,
                    stage,
                    outcome: None,
                    metrics: None,
                    message: Some(message.into()),
                },
            },
        }),
    )
}

pub(crate) fn context_compaction_delta_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    stage: ContextCompactionStage,
    outcome: Option<ContextCompactionOutcome>,
    message: impl Into<String>,
) -> TimelineEventEnvelope {
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemDelta,
        AgentEventPayload::ItemDelta(talos_sdk::TalosItemDeltaEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: "context-compaction".to_string(),
            delta: TalosTimelineDelta::ContextCompaction {
                stage: Some(stage),
                outcome,
                metrics: None,
                message: Some(message.into()),
            },
        }),
    )
}

pub(crate) fn tool_started_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    call_id: impl Into<String>,
    name: impl Into<String>,
    arguments: serde_json::Value,
) -> TimelineEventEnvelope {
    let call_id = call_id.into();
    let name = name.into();
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemStarted,
        AgentEventPayload::ItemStarted(talos_sdk::TalosItemStartedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item: TalosTimelineItem {
                item_id: format!("tool-{call_id}"),
                parent_item_id: None,
                kind: TalosTimelineItemKind::ToolCall,
                phase: TalosTimelinePhase::Acting,
                status: TalosTimelineItemStatus::Running,
                title: Some(name.clone()),
                started_at_ms: sequence,
                completed_at_ms: None,
                content: TalosTimelineContent::ToolCall {
                    call_id,
                    name,
                    arguments: arguments.to_string(),
                    progress: Vec::new(),
                    result: None,
                },
            },
        }),
    )
}

pub(crate) fn tool_completed_event(
    message_id: impl Into<String>,
    session_id: impl Into<String>,
    sequence: u64,
    call_id: impl Into<String>,
    name: impl Into<String>,
    output: impl Into<String>,
    is_error: bool,
) -> TimelineEventEnvelope {
    let call_id = call_id.into();
    let name = name.into();
    timeline_event(
        message_id.into(),
        session_id.into(),
        sequence,
        AgentEventType::ItemCompleted,
        AgentEventPayload::ItemCompleted(talos_sdk::TalosItemCompletedEvent {
            request_id: "req-test".to_string(),
            turn_id: "turn-test".to_string(),
            item_id: format!("tool-{call_id}"),
            status: if is_error {
                TalosTimelineItemStatus::Failed
            } else {
                TalosTimelineItemStatus::Completed
            },
            completed_at_ms: sequence,
            final_content: Some(TalosTimelineContent::ToolCall {
                call_id,
                name,
                arguments: "{}".to_string(),
                progress: Vec::new(),
                result: Some(talos_sdk::TalosToolResult {
                    content: vec![TalosContentItem::Text {
                        text: output.into(),
                    }],
                    is_error,
                }),
            }),
        }),
    )
}

pub(crate) fn timeline_event(
    message_id: String,
    session_id: String,
    sequence: u64,
    event_type: AgentEventType,
    payload: AgentEventPayload,
) -> TimelineEventEnvelope {
    let event_id = format!("ev_{sequence}");
    let event_seq = sequence as i64;
    let created_at_ms = sequence as i64;
    let event = AgentEvent {
        event_seq: Some(event_seq),
        event_id: Some(event_id.clone()),
        created_at_ms: Some(created_at_ms),
        event_type,
        turn_id: "turn-test".to_string(),
        message_id: message_id.clone(),
        session_id: session_id.clone(),
        sequence,
        payload,
    };
    TimelineEventEnvelope {
        event_id,
        event_seq,
        created_at_ms,
        session_id,
        turn_id: "turn-test".to_string(),
        message_id,
        event,
    }
}

impl MockGatewayClient {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockGatewayState {
                models: vec![
                    mock_model_info("qwen-plus", 128_000),
                    mock_model_info("gpt-5.3 medium", 128_000),
                ],
                session_id: "sess-test".to_string(),
                current_model: "qwen-plus".to_string(),
                current_project_root: None,
                auth_email: Some("talos@example.com".to_string()),
                workspace_root: PathBuf::from("/tmp/talos-workspace"),
                updated_model: None,
                stream_counter: 0,
                list_models_failures_remaining: 0,
                connect_delay_ms: 0,
                stream_failures_remaining: 0,
                close_session_delay_ms: 0,
                close_session_failures_remaining: 0,
                shutdown_calls: 0,
                cancelled_sessions: Vec::new(),
                session_usage: talos_types::talos::TokenUsage::default(),
                compact_calls: 0,
                clear_calls: 0,
                reconnect_calls: 0,
                reconnect_since_event_ids: Vec::new(),
                reconnect_events: Vec::new(),
                sent_requests: Vec::new(),
            })),
        }
    }

    pub(crate) fn with_connect_delay(connect_delay_ms: u64) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().connect_delay_ms = connect_delay_ms;
        client
    }

    pub(crate) fn with_list_models_failures(list_models_failures_remaining: u8) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().list_models_failures_remaining =
            list_models_failures_remaining;
        client
    }

    pub(crate) fn with_stream_failures(stream_failures_remaining: u8) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().stream_failures_remaining = stream_failures_remaining;
        client
    }

    pub(crate) fn with_active_reconnect_events(events: Vec<TimelineEventEnvelope>) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().reconnect_events = events;
        client
    }

    pub(crate) fn with_close_delay(close_session_delay_ms: u64) -> Self {
        let client = Self::new();
        client.state.lock().unwrap().close_session_delay_ms = close_session_delay_ms;
        client
    }
}

#[async_trait]
impl GatewayClient for MockGatewayClient {
    async fn list_models(&self) -> TalosResult<Vec<ModelInfo>> {
        let mut state = self.state.lock().unwrap();
        if state.list_models_failures_remaining > 0 {
            state.list_models_failures_remaining -= 1;
            return Err(TalosError::ClientError(
                "mock list_models failure".to_string(),
            ));
        }
        Ok(state.models.clone())
    }

    async fn read_auth_session(&self) -> TalosResult<AuthSessionReadResponse> {
        let state = self.state.lock().unwrap();
        Ok(AuthSessionReadResponse {
            session: Some(AuthSessionSnapshot {
                schema_version: 1,
                cloud_api_base_url: "https://cloud.example.test".to_string(),
                access_token: "test-token".to_string(),
                token_type: "Bearer".to_string(),
                expires_at_ms: None,
                last_used_at_ms: "1".to_string(),
                user: AuthSessionUserSnapshot {
                    user_id: "test-user".to_string(),
                    username: "talos-user".to_string(),
                    email: state.auth_email.clone(),
                    role: UserRole::User,
                    r#type: Default::default(),
                },
            }),
        })
    }

    async fn workspace_info(&self) -> TalosResult<WorkspaceInfoResponse> {
        let state = self.state.lock().unwrap();
        Ok(WorkspaceInfoResponse {
            user_id: "test-user".to_string(),
            username: Some("talos-user".to_string()),
            base_workspace_dir: state.workspace_root.to_string_lossy().to_string(),
            user_workspace_dir: state.workspace_root.to_string_lossy().to_string(),
        })
    }

    async fn suggest_skills(&self, _query: &str) -> TalosResult<Vec<SkillSuggestion>> {
        Ok(Vec::new())
    }

    async fn list_sessions(&self) -> TalosResult<Vec<SessionInfo>> {
        Ok(vec![])
    }

    async fn get_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        let state = self.state.lock().unwrap();
        Ok(state.session_info(3))
    }

    async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
            let should_fail = if state.close_session_failures_remaining > 0 {
                state.close_session_failures_remaining -= 1;
                true
            } else {
                false
            };
            (state.close_session_delay_ms, should_fail)
        };
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if should_fail {
            return Err(TalosError::ClientError(
                "mock close session failure".to_string(),
            ));
        }
        Ok(())
    }

    async fn delete_session(&self, _session_id: &str) -> TalosResult<()> {
        let delay = self.state.lock().unwrap().close_session_delay_ms;
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        Ok(())
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.state
            .lock()
            .unwrap()
            .cancelled_sessions
            .push(session_id.to_string());
        Ok(())
    }

    async fn update_session_model(
        &self,
        _session_id: &str,
        model_ref: &talos_sdk::ModelRef,
    ) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.current_model = model_ref.id.clone();
        state.updated_model = Some(model_ref.id.clone());
        Ok(state.session_info(3))
    }

    async fn compact_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.compact_calls = state.compact_calls.saturating_add(1);
        state.session_usage.total_tokens = state.session_usage.total_tokens.saturating_add(42);
        Ok(state.session_info(3))
    }

    async fn clear_session(&self, _session_id: &str) -> TalosResult<SessionInfo> {
        let mut state = self.state.lock().unwrap();
        state.clear_calls = state.clear_calls.saturating_add(1);
        state.session_usage = talos_types::talos::TokenUsage::default();
        Ok(state.session_info(0))
    }

    async fn start_turn_stream(
        &self,
        request: StartTurnRequest,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
            state.sent_requests.push(request.clone());
            let should_fail = if state.stream_failures_remaining > 0 {
                state.stream_failures_remaining -= 1;
                true
            } else {
                false
            };
            (state.connect_delay_ms, should_fail)
        };
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if should_fail {
            return Err(TalosError::ClientError(
                "mock stream connect failure".to_string(),
            ));
        }

        let (tx, rx) = mpsc::channel(32);
        let prompt = request
            .input
            .iter()
            .find_map(|item| match item {
                TalosInputItem::Text { text } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "unknown".to_string());
        let session_id = request.session_id.clone();

        let message_id = {
            let mut state = self.state.lock().unwrap();
            state.stream_counter += 1;
            format!("msg-{}", state.stream_counter)
        };

        tx.send(started_event(message_id.clone(), 0)).await.unwrap();
        tx.send(output_item_started_event(
            message_id.clone(),
            session_id.to_string(),
            1,
        ))
        .await
        .unwrap();

        tx.send(output_delta_event(
            message_id.clone(),
            session_id.to_string(),
            2,
            "# Title\n",
        ))
        .await
        .unwrap();

        tx.send(output_delta_event(
            message_id.clone(),
            session_id.to_string(),
            3,
            "- item\n",
        ))
        .await
        .unwrap();

        tx.send(output_delta_event(
            message_id.clone(),
            session_id.to_string(),
            4,
            format!("reply({})", prompt),
        ))
        .await
        .unwrap();

        let final_text = format!("# Title\n- item\nreply({prompt})");
        tx.send(output_completed_event(
            message_id.clone(),
            session_id.to_string(),
            5,
            final_text,
        ))
        .await
        .unwrap();

        tx.send(completed_event(
            message_id,
            session_id.to_string(),
            6,
            TokenUsage::default(),
        ))
        .await
        .unwrap();

        drop(tx);
        Ok(rx)
    }

    async fn reconnect_active_turn_stream(
        &self,
        _session_id: &str,
        since_event_id: Option<String>,
    ) -> TalosResult<mpsc::Receiver<TimelineEventEnvelope>> {
        let events = {
            let mut state = self.state.lock().unwrap();
            state.reconnect_calls = state.reconnect_calls.saturating_add(1);
            state.reconnect_since_event_ids.push(since_event_id.clone());
            state.reconnect_events.clone()
        };
        if events.is_empty() {
            return Err(TalosError::Conflict(
                "No active turn stream is available for this session.".to_string(),
            ));
        }
        let (tx, rx) = mpsc::channel(32);
        for event in events {
            tx.send(event).await.unwrap();
        }
        drop(tx);
        Ok(rx)
    }

    async fn get_session_history(&self, session_id: &str) -> TalosResult<SessionHistorySnapshot> {
        Ok(SessionHistorySnapshot {
            session_id: session_id.to_string(),
            messages: Vec::new(),
            generated_at_ms: 0,
            last_stable_event_id: None,
        })
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<TimelineActiveTurnSnapshot> {
        Ok(TimelineActiveTurnSnapshot {
            session_id: session_id.to_string(),
            status: TimelineActiveTurnStatus::Completed,
            turn_id: None,
            message_id: None,
            last_event_id: None,
            pending_user_action: None,
            updated_at: 0,
        })
    }

    async fn recover_timeline(
        &self,
        _session_id: &str,
        _after_event_id: Option<String>,
    ) -> TalosResult<TimelineRecoveryResponse> {
        Ok(TimelineRecoveryResponse {
            snapshot: TimelineRecoverySnapshot {
                replay: TimelineReplay {
                    events: Vec::new(),
                    high_water_mark_event_id: None,
                    last_replay_seq: 0,
                    cursor_status: TimelineCursorStatus::FromBeginning,
                },
                active_turn: TimelineActiveTurnSnapshot {
                    session_id: _session_id.to_string(),
                    status: TimelineActiveTurnStatus::Completed,
                    turn_id: None,
                    message_id: None,
                    last_event_id: None,
                    pending_user_action: None,
                    updated_at: 0,
                },
            },
        })
    }

    async fn shutdown(&self) -> TalosResult<()> {
        let mut state = self.state.lock().unwrap();
        state.shutdown_calls = state.shutdown_calls.saturating_add(1);
        Ok(())
    }
}

pub(crate) fn create_embedded_app() -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app() -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::new());
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_connect_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_connect_delay(delay_ms));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_model_list_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_list_models_failures(failures));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_stream_failures(failures: u8) -> TuiApp {
    let client: Arc<dyn GatewayClient> =
        Arc::new(MockGatewayClient::with_stream_failures(failures));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn create_app_server_app_with_close_delay(delay_ms: u64) -> TuiApp {
    let client: Arc<dyn GatewayClient> = Arc::new(MockGatewayClient::with_close_delay(delay_ms));
    TuiApp::new_with_gateway_client(client)
}

pub(crate) fn set_test_model_info(app: &mut TuiApp, model_id: &str, context_window: u32) {
    app.available_model_info = vec![mock_model_info(model_id, context_window)];
    app.available_models = vec![model_id.to_string()];
}

pub(crate) fn line_plain_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>()
}

pub(crate) fn line_display_width(line: &ratatui::text::Line<'_>) -> usize {
    line.spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(char_display_width)
        .sum()
}
