use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageCommandStatus {
    Pending,
    Accepted,
    Failed,
    FailedNoFirstEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageCommandAck {
    pub request_id: String,
    pub message_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub status: MessageCommandStatus,
}
