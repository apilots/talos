use crate::model::ModelSelection;
use crate::{AgentUserInput, UserPermissionMode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
/// Request payload used by clients to start a turn in a session.
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct StartTurnRequest {
    pub session_id: String,
    pub input: AgentUserInput,
    #[serde(default)]
    #[ts(type = "ModelSelection | null")]
    /// Explicit model selection for this turn; absent means use the session or configured default.
    pub model_selection: Option<ModelSelection>,
    #[serde(default)]
    #[ts(type = "UserPermissionMode | null")]
    /// Permission override used only when this request creates a missing session.
    pub user_permission_mode: Option<UserPermissionMode>,
    #[serde(default)]
    #[ts(type = "string | null")]
    pub project_id: Option<String>,
    #[serde(default)]
    #[ts(type = "string | null")]
    pub current_directory: Option<PathBuf>,
    #[serde(default)]
    #[ts(type = "string | null")]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct RetryTurnRequest {
    pub session_id: String,
    pub input: AgentUserInput,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SubscribeTimelineRequest {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub since_event_id: Option<String>,
    #[serde(default)]
    pub replay_limit: Option<usize>,
    #[serde(default)]
    pub close_when_idle: Option<bool>,
    #[serde(default)]
    pub close_on_terminal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct RecoverTimelineRequest {
    pub session_id: String,
    #[serde(default)]
    pub after_event_id: Option<String>,
    #[serde(default)]
    pub replay_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SubscribeTimelineResponse {
    pub subscription_id: String,
    pub session_id: String,
    #[serde(default)]
    pub high_water_mark_event_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum TurnCommandStatus {
    Pending,
    Accepted,
    Failed,
    FailedNoFirstEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct StartTurnResponse {
    pub request_id: String,
    #[serde(default)]
    pub message_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub status: TurnCommandStatus,
}
