use crate::{
    execution::EventCompaction, EventActions, EventAuthor, EventPayload, ExecutionBranch,
    ExecutionEvent, ExecutionEventKind, StateNamespace,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, FunctionCall};
use talos_memory::SessionTranscriptPayload;
use talos_memory::{
    Memory, MemoryStore, SessionTranscriptActions, SessionTranscriptApprovalResponsePayload,
    SessionTranscriptAuthPayload, SessionTranscriptAuthor, SessionTranscriptBranch,
    SessionTranscriptCompaction, SessionTranscriptCompactionPayload,
    SessionTranscriptConfirmationPayload, SessionTranscriptDynamicToolRequestPayload,
    SessionTranscriptDynamicToolResponsePayload, SessionTranscriptEventKind,
    SessionTranscriptHeader, SessionTranscriptMapper, SessionTranscriptRecord,
    SessionTranscriptStateDelta, SessionTranscriptStateNamespace, SessionTranscriptTransferPayload,
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    UserMessage,
    AgentMessage,
    ToolCall,
    ToolResult,
    SystemMessage,
    Planning,
    StateUpdate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEventPayload {
    Message {
        message: ChatMessage,
    },
    StateOnly,
    Transfer {
        payload: SessionTransferPayload,
    },
    Auth {
        payload: SessionAuthPayload,
    },
    Confirmation {
        payload: SessionConfirmationPayload,
    },
    ApprovalResponse {
        payload: SessionApprovalResponsePayload,
    },
    DynamicToolRequest {
        payload: SessionDynamicToolRequestPayload,
    },
    DynamicToolResponse {
        payload: SessionDynamicToolResponsePayload,
    },
    Compaction {
        payload: SessionCompactionPayload,
    },
}

impl SessionEventPayload {
    pub fn message(&self) -> Option<&ChatMessage> {
        match self {
            Self::Message { message } => Some(message),
            Self::Compaction { payload } => Some(&payload.summary_message),
            Self::Transfer { .. }
            | Self::Auth { .. }
            | Self::Confirmation { .. }
            | Self::ApprovalResponse { .. }
            | Self::DynamicToolRequest { .. }
            | Self::DynamicToolResponse { .. }
            | Self::StateOnly => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTransferPayload {
    pub target_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionAuthPayload {
    pub provider: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionConfirmationPayload {
    pub tool_name: String,
    pub call_id: String,
    pub prompt: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionApprovalResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionDynamicToolRequestPayload {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionDynamicToolResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionCompactionPayload {
    pub summary_message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: String,
    pub session_id: String,
    pub invocation_id: String,
    pub branch: ExecutionBranch,
    pub author: EventAuthor,
    pub kind: SessionEventKind,
    pub payload: SessionEventPayload,
    #[serde(default)]
    pub actions: EventActions,
    #[serde(default)]
    pub partial: bool,
    pub timestamp: DateTime<Utc>,
}

impl SessionEvent {
    pub fn new(
        session_id: impl Into<String>,
        invocation_id: impl Into<String>,
        branch: ExecutionBranch,
        author: EventAuthor,
        kind: SessionEventKind,
        payload: SessionEventPayload,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            invocation_id: invocation_id.into(),
            branch,
            author,
            kind,
            payload,
            actions: EventActions::default(),
            partial: false,
            timestamp: Utc::now(),
        }
    }

    pub fn from_execution_event(session_id: impl Into<String>, event: ExecutionEvent) -> Self {
        let payload = match &event.payload {
            EventPayload::Text { text } => match event.kind {
                ExecutionEventKind::UserInput => SessionEventPayload::Message {
                    message: ChatMessage::user(text.clone()),
                },
                ExecutionEventKind::System | ExecutionEventKind::Error => {
                    SessionEventPayload::Message {
                        message: ChatMessage::system(text.clone()),
                    }
                }
                ExecutionEventKind::FinalResponse | ExecutionEventKind::Reasoning => {
                    SessionEventPayload::Message {
                        message: ChatMessage::assistant(text.clone()),
                    }
                }
                ExecutionEventKind::Planning => SessionEventPayload::StateOnly,
                ExecutionEventKind::ToolCall | ExecutionEventKind::ToolResult => {
                    SessionEventPayload::StateOnly
                }
            },
            EventPayload::ToolCall {
                call_id,
                name,
                arguments,
            } => SessionEventPayload::Message {
                message: ChatMessage::assistant_with_tool_calls(
                    "",
                    vec![FunctionCall::new(
                        call_id.clone(),
                        name.clone(),
                        arguments.clone(),
                    )],
                ),
            },
            EventPayload::ToolResult {
                call_id,
                tool_name,
                content,
                ..
            } => SessionEventPayload::Message {
                message: ChatMessage::tool(call_id.clone(), tool_name.clone(), content.to_string()),
            },
            EventPayload::AuthRequest { payload } => SessionEventPayload::Auth {
                payload: SessionAuthPayload {
                    provider: payload.provider.clone(),
                    prompt: payload.prompt.clone(),
                },
            },
            EventPayload::ConfirmationRequest { payload } => SessionEventPayload::Confirmation {
                payload: SessionConfirmationPayload {
                    tool_name: payload.tool_name.clone(),
                    call_id: payload.call_id.clone(),
                    prompt: payload.prompt.clone(),
                    arguments: payload.arguments.clone(),
                },
            },
            EventPayload::ApprovalResponse { payload } => SessionEventPayload::ApprovalResponse {
                payload: SessionApprovalResponsePayload {
                    call_id: payload.call_id.clone(),
                    tool_name: payload.tool_name.clone(),
                    approved: payload.approved,
                    reason: payload.reason.clone(),
                },
            },
            EventPayload::DynamicToolRequest { payload } => {
                SessionEventPayload::DynamicToolRequest {
                    payload: SessionDynamicToolRequestPayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        arguments: payload.arguments.clone(),
                    },
                }
            }
            EventPayload::DynamicToolResponse { payload } => {
                SessionEventPayload::DynamicToolResponse {
                    payload: SessionDynamicToolResponsePayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        output: payload.output.clone(),
                        success: payload.success,
                    },
                }
            }
            EventPayload::CompactionSummary { payload } => SessionEventPayload::Compaction {
                payload: SessionCompactionPayload {
                    summary_message: ChatMessage::assistant(payload.summary.clone()),
                },
            },
            EventPayload::Empty => {
                if let Some(target_agent) = &event.actions.transfer_to {
                    SessionEventPayload::Transfer {
                        payload: SessionTransferPayload {
                            target_agent: target_agent.clone(),
                        },
                    }
                } else if event.actions.compaction.is_some() {
                    SessionEventPayload::Compaction {
                        payload: SessionCompactionPayload {
                            summary_message: ChatMessage::assistant(""),
                        },
                    }
                } else {
                    SessionEventPayload::StateOnly
                }
            }
        };

        let kind = match event.kind {
            ExecutionEventKind::UserInput => SessionEventKind::UserMessage,
            ExecutionEventKind::FinalResponse | ExecutionEventKind::Reasoning => {
                SessionEventKind::AgentMessage
            }
            ExecutionEventKind::ToolCall => SessionEventKind::ToolCall,
            ExecutionEventKind::ToolResult => SessionEventKind::ToolResult,
            ExecutionEventKind::Planning => SessionEventKind::Planning,
            ExecutionEventKind::System | ExecutionEventKind::Error => {
                SessionEventKind::SystemMessage
            }
        };

        Self {
            id: event.id,
            session_id: session_id.into(),
            invocation_id: event.invocation_id,
            branch: event.branch,
            author: event.author,
            kind,
            payload,
            actions: event.actions,
            partial: event.partial,
            timestamp: DateTime::<Utc>::from_timestamp_millis(event.timestamp_ms)
                .unwrap_or_else(Utc::now),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    #[serde(default)]
    pub bags: BTreeMap<StateNamespace, BTreeMap<String, Value>>,
}

impl SessionState {
    pub fn get(&self, namespace: StateNamespace, key: &str) -> Option<&Value> {
        self.bags.get(&namespace).and_then(|bag| bag.get(key))
    }

    pub fn bag(&self, namespace: StateNamespace) -> Option<&BTreeMap<String, Value>> {
        self.bags.get(&namespace)
    }

    pub fn insert(&mut self, namespace: StateNamespace, key: String, value: Value) {
        self.bags.entry(namespace).or_default().insert(key, value);
    }

    pub fn apply_delta(&mut self, delta: &crate::StateDelta) {
        self.insert(delta.namespace, delta.key.clone(), delta.value.clone());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvocationSession {
    pub session_id: String,
    #[serde(default)]
    pub events: Vec<SessionEvent>,
    #[serde(default)]
    pub state: SessionState,
}

impl InvocationSession {
    pub fn filter_events(
        &self,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> Vec<SessionEvent> {
        self.events
            .iter()
            .filter(|event| {
                invocation_id
                    .map(|expected| event.invocation_id == expected)
                    .unwrap_or(true)
                    && branch
                        .map(|expected| event.branch == expected)
                        .unwrap_or(true)
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionEventFilter {
    pub invocation_id: Option<String>,
    pub branch: Option<ExecutionBranch>,
}

#[async_trait]
pub trait SessionService: Send + Sync {
    async fn append_event(
        &self,
        session: &mut InvocationSession,
        event: SessionEvent,
    ) -> TalosResult<SessionEvent>;

    async fn load_session(
        &self,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<InvocationSession>;

    async fn list_events(
        &self,
        session_id: &str,
        filter: &SessionEventFilter,
    ) -> TalosResult<Vec<SessionEvent>>;
}

pub struct MemoryBackedSessionService {
    memory_store: Arc<MemoryStore>,
    mapper: Arc<dyn SessionTranscriptMapper<SessionEvent>>,
}

impl MemoryBackedSessionService {
    pub fn new(memory_store: Arc<MemoryStore>) -> Self {
        Self {
            memory_store,
            mapper: Arc::new(KernelSessionTranscriptMapper),
        }
    }
}

struct KernelSessionTranscriptMapper;

impl KernelSessionTranscriptMapper {
    fn map_branch_to_transcript(branch: ExecutionBranch) -> SessionTranscriptBranch {
        match branch {
            ExecutionBranch::DirectAnswer => SessionTranscriptBranch::DirectAnswer,
            ExecutionBranch::ReactiveLoop => SessionTranscriptBranch::ReactiveLoop,
            ExecutionBranch::ModelThinking => SessionTranscriptBranch::ModelThinking,
        }
    }

    fn map_branch_from_transcript(branch: SessionTranscriptBranch) -> ExecutionBranch {
        match branch {
            SessionTranscriptBranch::DirectAnswer => ExecutionBranch::DirectAnswer,
            SessionTranscriptBranch::ReactiveLoop => ExecutionBranch::ReactiveLoop,
            SessionTranscriptBranch::ModelThinking => ExecutionBranch::ModelThinking,
        }
    }

    fn map_author_to_transcript(author: EventAuthor) -> SessionTranscriptAuthor {
        match author {
            EventAuthor::User => SessionTranscriptAuthor::User,
            EventAuthor::Agent => SessionTranscriptAuthor::Agent,
            EventAuthor::Tool => SessionTranscriptAuthor::Tool,
            EventAuthor::System => SessionTranscriptAuthor::System,
        }
    }

    fn map_author_from_transcript(author: SessionTranscriptAuthor) -> EventAuthor {
        match author {
            SessionTranscriptAuthor::User => EventAuthor::User,
            SessionTranscriptAuthor::Agent => EventAuthor::Agent,
            SessionTranscriptAuthor::Tool => EventAuthor::Tool,
            SessionTranscriptAuthor::System => EventAuthor::System,
        }
    }

    fn map_kind_to_transcript(kind: SessionEventKind) -> SessionTranscriptEventKind {
        match kind {
            SessionEventKind::UserMessage => SessionTranscriptEventKind::UserMessage,
            SessionEventKind::AgentMessage => SessionTranscriptEventKind::AgentMessage,
            SessionEventKind::ToolCall => SessionTranscriptEventKind::ToolCall,
            SessionEventKind::ToolResult => SessionTranscriptEventKind::ToolResult,
            SessionEventKind::SystemMessage => SessionTranscriptEventKind::SystemMessage,
            SessionEventKind::Planning => SessionTranscriptEventKind::Planning,
            SessionEventKind::StateUpdate => SessionTranscriptEventKind::StateUpdate,
        }
    }

    fn map_kind_from_transcript(kind: SessionTranscriptEventKind) -> SessionEventKind {
        match kind {
            SessionTranscriptEventKind::UserMessage => SessionEventKind::UserMessage,
            SessionTranscriptEventKind::AgentMessage => SessionEventKind::AgentMessage,
            SessionTranscriptEventKind::ToolCall => SessionEventKind::ToolCall,
            SessionTranscriptEventKind::ToolResult => SessionEventKind::ToolResult,
            SessionTranscriptEventKind::SystemMessage => SessionEventKind::SystemMessage,
            SessionTranscriptEventKind::Planning => SessionEventKind::Planning,
            SessionTranscriptEventKind::StateUpdate => SessionEventKind::StateUpdate,
        }
    }

    fn map_actions_to_transcript(actions: &EventActions) -> SessionTranscriptActions {
        SessionTranscriptActions {
            state_delta: actions
                .state_delta
                .iter()
                .map(|delta| SessionTranscriptStateDelta {
                    namespace: match delta.namespace {
                        StateNamespace::App => SessionTranscriptStateNamespace::App,
                        StateNamespace::User => SessionTranscriptStateNamespace::User,
                        StateNamespace::Temp => SessionTranscriptStateNamespace::Temp,
                        StateNamespace::Session => SessionTranscriptStateNamespace::Session,
                        StateNamespace::Workspace => SessionTranscriptStateNamespace::Workspace,
                    },
                    key: delta.key.clone(),
                    value: delta.value.clone(),
                })
                .collect(),
            end_turn: actions.end_turn,
            transfer_to: actions.transfer_to.clone(),
            branch_override: actions.branch_override.map(Self::map_branch_to_transcript),
            rewind_before_invocation_id: actions.rewind_before_invocation_id.clone(),
            compaction: actions
                .compaction
                .as_ref()
                .map(|compaction| SessionTranscriptCompaction {
                    start_event_id: compaction.start_event_id.clone(),
                    end_event_id: compaction.end_event_id.clone(),
                }),
        }
    }

    fn map_actions_from_transcript(actions: SessionTranscriptActions) -> EventActions {
        EventActions {
            state_delta: actions
                .state_delta
                .into_iter()
                .map(|delta| crate::StateDelta {
                    namespace: match delta.namespace {
                        SessionTranscriptStateNamespace::App => StateNamespace::App,
                        SessionTranscriptStateNamespace::User => StateNamespace::User,
                        SessionTranscriptStateNamespace::Temp => StateNamespace::Temp,
                        SessionTranscriptStateNamespace::Session => StateNamespace::Session,
                        SessionTranscriptStateNamespace::Workspace => StateNamespace::Workspace,
                    },
                    key: delta.key,
                    value: delta.value,
                })
                .collect(),
            end_turn: actions.end_turn,
            transfer_to: actions.transfer_to,
            branch_override: actions
                .branch_override
                .map(Self::map_branch_from_transcript),
            rewind_before_invocation_id: actions.rewind_before_invocation_id,
            compaction: actions.compaction.map(|compaction| EventCompaction {
                start_event_id: compaction.start_event_id,
                end_event_id: compaction.end_event_id,
            }),
        }
    }

    fn apply_state_delta(session: &mut InvocationSession, event: &SessionEvent) {
        for delta in &event.actions.state_delta {
            session.state.apply_delta(delta);
        }
    }

    fn map_payload_to_transcript(payload: &SessionEventPayload) -> SessionTranscriptPayload {
        match payload {
            SessionEventPayload::Message { message } => SessionTranscriptPayload::Message {
                message: message.clone(),
            },
            SessionEventPayload::StateOnly => SessionTranscriptPayload::StateOnly,
            SessionEventPayload::Transfer { payload } => SessionTranscriptPayload::Transfer {
                payload: SessionTranscriptTransferPayload {
                    target_agent: payload.target_agent.clone(),
                },
            },
            SessionEventPayload::Auth { payload } => SessionTranscriptPayload::Auth {
                payload: SessionTranscriptAuthPayload {
                    provider: payload.provider.clone(),
                    prompt: payload.prompt.clone(),
                },
            },
            SessionEventPayload::Confirmation { payload } => {
                SessionTranscriptPayload::Confirmation {
                    payload: SessionTranscriptConfirmationPayload {
                        tool_name: payload.tool_name.clone(),
                        call_id: payload.call_id.clone(),
                        prompt: payload.prompt.clone(),
                        arguments: payload.arguments.clone(),
                    },
                }
            }
            SessionEventPayload::ApprovalResponse { payload } => {
                SessionTranscriptPayload::ApprovalResponse {
                    payload: SessionTranscriptApprovalResponsePayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        approved: payload.approved,
                        reason: payload.reason.clone(),
                    },
                }
            }
            SessionEventPayload::DynamicToolRequest { payload } => {
                SessionTranscriptPayload::DynamicToolRequest {
                    payload: SessionTranscriptDynamicToolRequestPayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        arguments: payload.arguments.clone(),
                    },
                }
            }
            SessionEventPayload::DynamicToolResponse { payload } => {
                SessionTranscriptPayload::DynamicToolResponse {
                    payload: SessionTranscriptDynamicToolResponsePayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        output: payload.output.clone(),
                        success: payload.success,
                    },
                }
            }
            SessionEventPayload::Compaction { payload } => SessionTranscriptPayload::Compaction {
                payload: SessionTranscriptCompactionPayload {
                    summary_message: payload.summary_message.clone(),
                },
            },
        }
    }

    fn map_payload_from_transcript(payload: SessionTranscriptPayload) -> SessionEventPayload {
        match payload {
            SessionTranscriptPayload::Message { message } => {
                SessionEventPayload::Message { message }
            }
            SessionTranscriptPayload::StateOnly => SessionEventPayload::StateOnly,
            SessionTranscriptPayload::Transfer { payload } => SessionEventPayload::Transfer {
                payload: SessionTransferPayload {
                    target_agent: payload.target_agent,
                },
            },
            SessionTranscriptPayload::Auth { payload } => SessionEventPayload::Auth {
                payload: SessionAuthPayload {
                    provider: payload.provider,
                    prompt: payload.prompt,
                },
            },
            SessionTranscriptPayload::Confirmation { payload } => {
                SessionEventPayload::Confirmation {
                    payload: SessionConfirmationPayload {
                        tool_name: payload.tool_name,
                        call_id: payload.call_id,
                        prompt: payload.prompt,
                        arguments: payload.arguments,
                    },
                }
            }
            SessionTranscriptPayload::ApprovalResponse { payload } => {
                SessionEventPayload::ApprovalResponse {
                    payload: SessionApprovalResponsePayload {
                        call_id: payload.call_id,
                        tool_name: payload.tool_name,
                        approved: payload.approved,
                        reason: payload.reason,
                    },
                }
            }
            SessionTranscriptPayload::DynamicToolRequest { payload } => {
                SessionEventPayload::DynamicToolRequest {
                    payload: SessionDynamicToolRequestPayload {
                        call_id: payload.call_id,
                        tool_name: payload.tool_name,
                        arguments: payload.arguments,
                    },
                }
            }
            SessionTranscriptPayload::DynamicToolResponse { payload } => {
                SessionEventPayload::DynamicToolResponse {
                    payload: SessionDynamicToolResponsePayload {
                        call_id: payload.call_id,
                        tool_name: payload.tool_name,
                        output: payload.output,
                        success: payload.success,
                    },
                }
            }
            SessionTranscriptPayload::Compaction { payload } => SessionEventPayload::Compaction {
                payload: SessionCompactionPayload {
                    summary_message: payload.summary_message,
                },
            },
        }
    }
}

impl SessionTranscriptMapper<SessionEvent> for KernelSessionTranscriptMapper {
    fn to_record(&self, event: &SessionEvent) -> SessionTranscriptRecord {
        SessionTranscriptRecord {
            header: SessionTranscriptHeader {
                id: event.id.clone(),
                session_id: event.session_id.clone(),
                invocation_id: event.invocation_id.clone(),
                branch: Self::map_branch_to_transcript(event.branch),
                timestamp: event.timestamp,
            },
            author: Self::map_author_to_transcript(event.author.clone()),
            kind: Self::map_kind_to_transcript(event.kind.clone()),
            payload: Self::map_payload_to_transcript(&event.payload),
            actions: Self::map_actions_to_transcript(&event.actions),
            partial: event.partial,
        }
    }

    fn from_record(&self, record: SessionTranscriptRecord) -> SessionEvent {
        SessionEvent {
            id: record.header.id,
            session_id: record.header.session_id,
            invocation_id: record.header.invocation_id,
            branch: Self::map_branch_from_transcript(record.header.branch),
            author: Self::map_author_from_transcript(record.author),
            kind: Self::map_kind_from_transcript(record.kind),
            payload: Self::map_payload_from_transcript(record.payload),
            actions: Self::map_actions_from_transcript(record.actions),
            partial: record.partial,
            timestamp: record.header.timestamp,
        }
    }
}

#[async_trait]
impl SessionService for MemoryBackedSessionService {
    async fn append_event(
        &self,
        session: &mut InvocationSession,
        event: SessionEvent,
    ) -> TalosResult<SessionEvent> {
        if event.partial {
            return Ok(event);
        }

        KernelSessionTranscriptMapper::apply_state_delta(session, &event);
        self.memory_store
            .append_transcript_record(&self.mapper.to_record(&event))
            .await?;
        session.events.push(event.clone());
        Ok(event)
    }

    async fn load_session(
        &self,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<InvocationSession> {
        let events = if let (Some(invocation_id), Some(branch)) = (invocation_id, branch) {
            self.memory_store
                .load_transcript_records(
                    session_id,
                    Some(invocation_id),
                    KernelSessionTranscriptMapper::map_branch_to_transcript(branch),
                )
                .await?
                .into_iter()
                .map(|record| self.mapper.from_record(record))
                .collect()
        } else if let Some(branch) = branch {
            self.memory_store
                .load_transcript_records(
                    session_id,
                    invocation_id,
                    KernelSessionTranscriptMapper::map_branch_to_transcript(branch),
                )
                .await?
                .into_iter()
                .map(|record| self.mapper.from_record(record))
                .collect()
        } else {
            Vec::new()
        };

        let mut session = InvocationSession {
            session_id: session_id.to_string(),
            events,
            state: SessionState::default(),
        };
        for event in session.events.clone() {
            KernelSessionTranscriptMapper::apply_state_delta(&mut session, &event);
        }
        Ok(session)
    }

    async fn list_events(
        &self,
        session_id: &str,
        filter: &SessionEventFilter,
    ) -> TalosResult<Vec<SessionEvent>> {
        let session = self
            .load_session(session_id, filter.invocation_id.as_deref(), filter.branch)
            .await?;
        Ok(session.filter_events(filter.invocation_id.as_deref(), filter.branch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_memory::MemoryStore;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_load_session_replays_full_branch_history_without_invocation_filter() {
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory_store = Arc::new(MemoryStore::new(workspace_dir));
        let service = MemoryBackedSessionService::new(memory_store);
        let mut session = InvocationSession {
            session_id: "session-1".to_string(),
            ..InvocationSession::default()
        };

        service
            .append_event(
                &mut session,
                SessionEvent::new(
                    "session-1",
                    "invocation-1",
                    ExecutionBranch::ReactiveLoop,
                    EventAuthor::User,
                    SessionEventKind::UserMessage,
                    SessionEventPayload::Message {
                        message: ChatMessage::user("hello"),
                    },
                ),
            )
            .await
            .expect("first append should succeed");
        service
            .append_event(
                &mut session,
                SessionEvent::new(
                    "session-1",
                    "invocation-2",
                    ExecutionBranch::ReactiveLoop,
                    EventAuthor::Agent,
                    SessionEventKind::AgentMessage,
                    SessionEventPayload::Message {
                        message: ChatMessage::assistant("hi again"),
                    },
                ),
            )
            .await
            .expect("second append should succeed");

        let loaded = service
            .load_session("session-1", None, Some(ExecutionBranch::ReactiveLoop))
            .await
            .expect("load should succeed");

        assert_eq!(loaded.events.len(), 2);
        assert_eq!(loaded.events[0].invocation_id, "invocation-1");
        assert_eq!(loaded.events[1].invocation_id, "invocation-2");
    }

    #[tokio::test]
    async fn test_load_session_replays_state_delta_into_session_state() {
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory_store = Arc::new(MemoryStore::new(workspace_dir));
        let service = MemoryBackedSessionService::new(memory_store);
        let mut session = InvocationSession {
            session_id: "session-1".to_string(),
            ..InvocationSession::default()
        };

        let mut event = SessionEvent::new(
            "session-1",
            "invocation-1",
            ExecutionBranch::ReactiveLoop,
            EventAuthor::Agent,
            SessionEventKind::Planning,
            SessionEventPayload::StateOnly,
        );
        event.actions.state_delta.push(crate::StateDelta {
            namespace: StateNamespace::Session,
            key: "plan_seen".to_string(),
            value: serde_json::json!(true),
        });

        service
            .append_event(&mut session, event)
            .await
            .expect("append should succeed");

        let loaded = service
            .load_session("session-1", None, Some(ExecutionBranch::ReactiveLoop))
            .await
            .expect("load should succeed");

        assert_eq!(
            loaded.state.get(StateNamespace::Session, "plan_seen"),
            Some(&serde_json::json!(true))
        );
    }
}
