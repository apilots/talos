//! Provider-neutral messages and process-local input provenance.
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::{Attachment, FunctionCall, ReasoningContinuation};

/// Provider-neutral chat message.
///
/// Construct messages with [`ChatMessage::system`], [`ChatMessage::user`],
/// [`ChatMessage::user_with_attachments`], [`ChatMessage::assistant`], or
/// [`ChatMessage::tool`]. This keeps role-specific invariants explicit and is
/// the supported construction path for downstream crates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// Persisted FileService references for tool images; re-resolved before model calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_file_ids: Vec<String>,
    /// Ordered binary inputs for a user or tool message.
    ///
    /// `None` is the only no-attachment state. Generic message serialization
    /// skips this field; shared protocol encoders are its only wire owners.
    #[serde(skip, default)]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<FunctionCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_continuation: Option<ReasoningContinuation>,
    // Only RequestLevel providers should consume this field. Unsupported providers must drop it
    // during request conversion rather than serializing it into outbound payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
    /// Process-local provenance supplied by the actual input owner. It is not
    /// Provider wire data or persisted conversation state.
    #[serde(skip)]
    pub input_origin: Option<MessageInputOrigin>,
}

/// Message-level provenance, independent of Agent observation transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageInputOrigin {
    pub kind: MessageInputKind,
    pub source: MessageInputSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageInputKind {
    System,
    User,
    Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageInputSource {
    User,
    Prompt,
    Runtime,
    Workspace,
    Skills,
    Memory,
    Mixed,
}

// Observation metadata must not change conversation equality or cache decisions.
impl PartialEq for ChatMessage {
    fn eq(&self, other: &Self) -> bool {
        self.role == other.role
            && self.content == other.content
            && self.image_file_ids == other.image_file_ids
            && self.attachments == other.attachments
            && self.name == other.name
            && self.tool_call_id == other.tool_call_id
            && self.tool_calls == other.tool_calls
            && self.reasoning_continuation == other.reasoning_continuation
            && self.cache_control == other.cache_control
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: CacheControlKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheControlKind {
    #[serde(rename = "ephemeral")]
    Ephemeral,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
            input_origin: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
            input_origin: None,
        }
    }

    /// Creates a user message with one or more ordered attachments.
    pub fn user_with_attachments(
        content: impl Into<String>,
        attachments: Vec<Attachment>,
    ) -> TalosResult<Self> {
        if attachments.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "attached user message must include at least one attachment".to_string(),
            ));
        }
        Ok(Self {
            attachments: Some(attachments),
            image_file_ids: Vec::new(),
            ..Self::user(content)
        })
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
            input_origin: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<FunctionCall>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(tool_calls),
            reasoning_continuation: None,
            cache_control: None,
            input_origin: None,
        }
    }

    pub fn tool(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: Some(name.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
            input_origin: None,
        }
    }

    /// Attaches provider continuation required to replay this assistant message.
    pub fn with_reasoning_continuation(mut self, continuation: ReasoningContinuation) -> Self {
        self.reasoning_continuation = Some(continuation);
        self
    }

    pub fn with_input_origin(mut self, origin: MessageInputOrigin) -> Self {
        self.input_origin = Some(origin);
        self
    }
}

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}
