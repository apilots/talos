use crate::{
    EventActions, EventAuthor, EventPayload, ExecutionBranch, ExecutionEvent, ExecutionEventKind,
    StateNamespace,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, FunctionCall, ReasoningContinuation};
use talos_types::talos::DynamicToolArguments;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KernelTranscriptEventKind {
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
pub enum KernelTranscriptPayload {
    Message {
        message: ChatMessage,
    },
    StateOnly,
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
            | Self::StateOnly => None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelTranscriptEvent {
    pub id: String,
    pub session_id: String,
    pub invocation_id: String,
    pub branch: ExecutionBranch,
    pub author: EventAuthor,
    pub kind: KernelTranscriptEventKind,
    pub payload: KernelTranscriptPayload,
    #[serde(default)]
    pub actions: EventActions,
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
        kind: KernelTranscriptEventKind,
        payload: KernelTranscriptPayload,
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
                ExecutionEventKind::UserInput => KernelTranscriptPayload::Message {
                    message: ChatMessage::user(text.clone()),
                },
                ExecutionEventKind::System | ExecutionEventKind::Error => {
                    KernelTranscriptPayload::Message {
                        message: ChatMessage::system(text.clone()),
                    }
                }
                ExecutionEventKind::FinalResponse | ExecutionEventKind::Reasoning => {
                    KernelTranscriptPayload::Message {
                        message: ChatMessage::assistant(text.clone()),
                    }
                }
                ExecutionEventKind::Planning => KernelTranscriptPayload::StateOnly,
                ExecutionEventKind::ToolCall | ExecutionEventKind::ToolResult => {
                    KernelTranscriptPayload::StateOnly
                }
            },
            EventPayload::ToolCall {
                call_id,
                name,
                arguments,
            } => KernelTranscriptPayload::Message {
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
            } => KernelTranscriptPayload::Message {
                message: ChatMessage::tool(call_id.clone(), tool_name.clone(), content.to_string()),
            },
            EventPayload::AuthRequest { payload } => KernelTranscriptPayload::Auth {
                payload: KernelTranscriptAuthPayload {
                    provider: payload.provider.clone(),
                    prompt: payload.prompt.clone(),
                },
            },
            EventPayload::DynamicToolRequest { payload } => {
                KernelTranscriptPayload::UserActionRequest {
                    payload: KernelTranscriptUserActionRequestPayload {
                        action_id: format!("ua_{}", Uuid::now_v7().simple()),
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        arguments: payload.arguments.clone().into(),
                        params: talos_types::UserActionParams::DynamicToolResult(
                            talos_types::DynamicToolResultParams {
                                tool_name: payload.tool_name.clone(),
                            },
                        ),
                        sandbox_request_fingerprint: None,
                        reasoning_continuation: None,
                        tool_calls: vec![FunctionCall::new(
                            payload.call_id.clone(),
                            payload.tool_name.clone(),
                            payload.arguments.clone(),
                        )],
                        tool_messages: Vec::new(),
                        deferred_tool_calls: Vec::new(),
                    },
                }
            }
            EventPayload::DynamicToolResponse { payload } => {
                KernelTranscriptPayload::UserActionResponse {
                    payload: KernelTranscriptUserActionResponsePayload {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        response: talos_types::UserActionResponse::DynamicToolResult(
                            talos_types::DynamicToolResult {
                                output: payload.output.clone(),
                                success: payload.success,
                            },
                        ),
                    },
                }
            }
            EventPayload::CompactionSummary { payload } => KernelTranscriptPayload::Compaction {
                payload: KernelTranscriptCompactionPayload {
                    summary_message: ChatMessage::assistant(payload.summary.clone()),
                },
            },
            EventPayload::Empty => {
                if let Some(target_agent) = &event.actions.transfer_to {
                    KernelTranscriptPayload::Transfer {
                        payload: KernelTranscriptTransferPayload {
                            target_agent: target_agent.clone(),
                        },
                    }
                } else if event.actions.compaction.is_some() {
                    KernelTranscriptPayload::Compaction {
                        payload: KernelTranscriptCompactionPayload {
                            summary_message: ChatMessage::assistant(""),
                        },
                    }
                } else {
                    KernelTranscriptPayload::StateOnly
                }
            }
        };

        let kind = match event.kind {
            ExecutionEventKind::UserInput => KernelTranscriptEventKind::UserMessage,
            ExecutionEventKind::FinalResponse | ExecutionEventKind::Reasoning => {
                KernelTranscriptEventKind::AgentMessage
            }
            ExecutionEventKind::ToolCall => KernelTranscriptEventKind::ToolCall,
            ExecutionEventKind::ToolResult => KernelTranscriptEventKind::ToolResult,
            ExecutionEventKind::Planning => KernelTranscriptEventKind::Planning,
            ExecutionEventKind::System | ExecutionEventKind::Error => {
                KernelTranscriptEventKind::SystemMessage
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
    pub events: Vec<KernelTranscriptEvent>,
    #[serde(default)]
    pub state: SessionState,
}

impl InvocationSession {
    pub fn filter_events(
        &self,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> Vec<KernelTranscriptEvent> {
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
pub struct KernelTranscriptFilter {
    pub invocation_id: Option<String>,
    pub branch: Option<ExecutionBranch>,
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

    async fn list_events(
        &self,
        session_id: &str,
        filter: &KernelTranscriptFilter,
    ) -> TalosResult<Vec<KernelTranscriptEvent>>;
}
