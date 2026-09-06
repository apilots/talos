use crate::{EventAuthor, ExecutionBranch, SessionCompactionState, SessionStateUpdate};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, FunctionCall, ReasoningContinuation};
use talos_types::talos::DynamicToolArguments;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KernelTranscriptPayload {
    Message {
        message: ChatMessage,
    },
    SessionState {
        update: SessionStateUpdate,
    },
    Transfer {
        payload: KernelTranscriptTransferPayload,
    },
    Auth {
        payload: KernelTranscriptAuthPayload,
    },
    UserActionRequest {
        payload: KernelTranscriptUserActionRequestPayload,
    },
    UserActionResponse {
        payload: KernelTranscriptUserActionResponsePayload,
    },
    Compaction {
        payload: KernelTranscriptCompactionPayload,
    },
}

impl KernelTranscriptPayload {
    pub fn message(&self) -> Option<&ChatMessage> {
        match self {
            Self::Message { message } => Some(message),
            Self::Compaction { payload } => Some(&payload.summary_message),
            Self::Transfer { .. }
            | Self::Auth { .. }
            | Self::UserActionRequest { .. }
            | Self::UserActionResponse { .. }
            | Self::SessionState { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelTranscriptTransferPayload {
    pub target_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelTranscriptAuthPayload {
    pub provider: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KernelTranscriptUserActionRequestPayload {
    pub action_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: DynamicToolArguments,
    pub params: talos_types::UserActionParams,
    pub sandbox_request_fingerprint: Option<String>,
    pub reasoning_continuation: Option<ReasoningContinuation>,
    pub tool_calls: Vec<FunctionCall>,
    pub tool_messages: Vec<ChatMessage>,
    pub deferred_tool_calls: Vec<FunctionCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelTranscriptUserActionResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub response: talos_types::UserActionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KernelTranscriptCompactionPayload {
    pub summary_message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KernelTranscriptEvent {
    pub id: String,
    pub session_id: String,
    pub invocation_id: String,
    pub branch: ExecutionBranch,
    pub author: EventAuthor,
    pub payload: KernelTranscriptPayload,
    #[serde(default)]
    pub partial: bool,
    pub timestamp: DateTime<Utc>,
}

impl KernelTranscriptEvent {
    pub fn new(
        session_id: impl Into<String>,
        invocation_id: impl Into<String>,
        branch: ExecutionBranch,
        author: EventAuthor,
        payload: KernelTranscriptPayload,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            invocation_id: invocation_id.into(),
            branch,
            author,
            payload,
            partial: false,
            timestamp: Utc::now(),
        }
    }
}

/// Provider-private state reconstructed from typed transcript actions.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub prompt_warning: Option<talos_prompt::PromptWarningState>,
    pub compaction: Option<SessionCompactionState>,
    pub continuity: Option<talos_prompt::SessionContinuity>,
}

impl SessionState {
    pub fn apply(&mut self, update: &SessionStateUpdate) {
        match update {
            SessionStateUpdate::PromptWarning(state) => self.prompt_warning = Some(state.clone()),
            SessionStateUpdate::Compaction(state) => self.compaction = Some(state.clone()),
            SessionStateUpdate::Continuity(state) => self.continuity = Some(state.clone()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvocationSession {
    pub session_id: String,
    #[serde(default)]
    pub events: Vec<KernelTranscriptEvent>,
    #[serde(default)]
    pub state: SessionState,
}

#[async_trait]
pub trait KernelTranscriptService: Send + Sync {
    async fn append_event(
        &self,
        session: &mut InvocationSession,
        event: KernelTranscriptEvent,
    ) -> TalosResult<KernelTranscriptEvent>;

    async fn load_session(
        &self,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<InvocationSession>;
}
