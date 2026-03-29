use crate::{EventActions, EventAuthor, ExecutionBranch, StateNamespace};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, Role};
use talos_memory::{
    Memory, MemoryStore, SessionTranscriptBranch, SessionTranscriptHeader, SessionTranscriptRecord,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub id: String,
    pub session_id: String,
    pub invocation_id: String,
    pub branch: ExecutionBranch,
    pub author: EventAuthor,
    pub kind: SessionEventKind,
    pub message: Option<ChatMessage>,
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
        message: Option<ChatMessage>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            invocation_id: invocation_id.into(),
            branch,
            author,
            kind,
            message,
            actions: EventActions::default(),
            partial: false,
            timestamp: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    #[serde(default)]
    pub app_state: HashMap<String, Value>,
    #[serde(default)]
    pub user_state: HashMap<String, Value>,
    #[serde(default)]
    pub temp_state: HashMap<String, Value>,
    #[serde(default)]
    pub session_state: HashMap<String, Value>,
    #[serde(default)]
    pub workspace_state: HashMap<String, Value>,
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
}

impl MemoryBackedSessionService {
    pub fn new(memory_store: Arc<MemoryStore>) -> Self {
        Self { memory_store }
    }

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

    fn infer_kind(message: &ChatMessage) -> SessionEventKind {
        match message.role {
            Role::User => SessionEventKind::UserMessage,
            Role::System => SessionEventKind::SystemMessage,
            Role::Tool => SessionEventKind::ToolResult,
            Role::Assistant => {
                if message
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty())
                {
                    SessionEventKind::ToolCall
                } else {
                    SessionEventKind::AgentMessage
                }
            }
        }
    }

    fn apply_state_delta(session: &mut InvocationSession, event: &SessionEvent) {
        for delta in &event.actions.state_delta {
            match delta.namespace {
                StateNamespace::App => {
                    session
                        .state
                        .app_state
                        .insert(delta.key.clone(), delta.value.clone());
                }
                StateNamespace::User => {
                    session
                        .state
                        .user_state
                        .insert(delta.key.clone(), delta.value.clone());
                }
                StateNamespace::Temp => {
                    session
                        .state
                        .temp_state
                        .insert(delta.key.clone(), delta.value.clone());
                }
                StateNamespace::Session => {
                    session
                        .state
                        .session_state
                        .insert(delta.key.clone(), delta.value.clone());
                }
                StateNamespace::Workspace => {
                    session
                        .state
                        .workspace_state
                        .insert(delta.key.clone(), delta.value.clone());
                }
            }
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

        Self::apply_state_delta(session, &event);
        if let Some(message) = &event.message {
            self.memory_store
                .append_transcript_record(&SessionTranscriptRecord {
                    header: SessionTranscriptHeader {
                        id: event.id.clone(),
                        session_id: event.session_id.clone(),
                        invocation_id: event.invocation_id.clone(),
                        branch: Self::map_branch_to_transcript(event.branch),
                        timestamp: event.timestamp,
                    },
                    message: message.clone(),
                })
                .await?;
        }
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
                    Self::map_branch_to_transcript(branch),
                )
                .await?
                .into_iter()
                .map(|record| SessionEvent {
                    id: record.header.id,
                    session_id: record.header.session_id,
                    invocation_id: record.header.invocation_id,
                    branch: Self::map_branch_from_transcript(record.header.branch),
                    author: match record.message.role {
                        Role::User => EventAuthor::User,
                        Role::Tool => EventAuthor::Tool,
                        Role::System => EventAuthor::System,
                        Role::Assistant => EventAuthor::Agent,
                    },
                    kind: Self::infer_kind(&record.message),
                    message: Some(record.message),
                    actions: EventActions::default(),
                    partial: false,
                    timestamp: record.header.timestamp,
                })
                .collect()
        } else if let Some(branch) = branch {
            self.memory_store
                .load_transcript_records(
                    session_id,
                    invocation_id,
                    Self::map_branch_to_transcript(branch),
                )
                .await?
                .into_iter()
                .map(|record| SessionEvent {
                    id: record.header.id,
                    session_id: record.header.session_id,
                    invocation_id: record.header.invocation_id,
                    branch: Self::map_branch_from_transcript(record.header.branch),
                    author: match record.message.role {
                        Role::User => EventAuthor::User,
                        Role::Tool => EventAuthor::Tool,
                        Role::System => EventAuthor::System,
                        Role::Assistant => EventAuthor::Agent,
                    },
                    kind: Self::infer_kind(&record.message),
                    message: Some(record.message),
                    actions: EventActions::default(),
                    partial: false,
                    timestamp: record.header.timestamp,
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(InvocationSession {
            session_id: session_id.to_string(),
            events,
            state: SessionState::default(),
        })
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
                    Some(ChatMessage::user("hello")),
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
                    Some(ChatMessage::assistant("hi again")),
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
}
