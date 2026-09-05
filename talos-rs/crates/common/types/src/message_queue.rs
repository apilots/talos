use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::AgentUserInput;

/// A user message waiting in a session-scoped in-memory queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct QueuedMessage {
    pub id: String,
    pub input: AgentUserInput,
}

/// The complete bounded message queue visible to clients for one session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueue {
    #[ts(type = "number")]
    pub revision: u64,
    pub max_messages: u32,
    pub messages: Vec<QueuedMessage>,
}

/// Live invalidation signal emitted after a visible queue mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueChangedNotification {
    pub session_id: String,
    #[ts(type = "number")]
    pub revision: u64,
}

/// Parameters for adding one message to a Session queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueAddRequest {
    pub session_id: String,
    pub input: AgentUserInput,
}

/// Parameters for reading one complete Session queue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueListRequest {
    pub session_id: String,
}

/// Parameters for replacing one waiting message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueUpdateRequest {
    pub session_id: String,
    pub message_id: String,
    pub input: AgentUserInput,
}

/// Parameters for deleting one waiting message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueDeleteRequest {
    pub session_id: String,
    pub message_id: String,
}

/// Parameters for applying a complete queue permutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueReorderRequest {
    pub session_id: String,
    pub message_ids: Vec<String>,
}

/// Parameters for starting the queue head or one selected message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct MessageQueueStartRequest {
    pub session_id: String,
    #[ts(optional, type = "string | null")]
    pub message_id: Option<String>,
}
