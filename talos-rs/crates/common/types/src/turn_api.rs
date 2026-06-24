use crate::talos::{TalosContext, TalosInputItem, TimelineEventEnvelope};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct StartTurnRequest {
    pub session_id: String,
    pub input: Vec<TalosInputItem>,
    #[serde(default)]
    #[ts(optional)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub context: Option<TalosContext>,
    #[serde(default)]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub project_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub current_directory: Option<PathBuf>,
    #[serde(default)]
    #[ts(optional)]
    pub client_request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct RetryTurnRequest {
    pub session_id: String,
    pub input: Vec<TalosInputItem>,
    #[serde(default)]
    #[ts(optional)]
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct InterruptTurnRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct InterruptTurnResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SubmitToolResultRequest {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SubmitToolResultResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SubscribeTimelineRequest {
    pub session_id: String,
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
pub struct RecoverTimelineRequest {
    pub session_id: String,
    #[serde(default)]
    pub after_event_id: Option<String>,
    #[serde(default)]
    pub replay_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineRecoverySnapshot {
    pub replay: TimelineReplay,
    pub active_turn: TimelineActiveTurnSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineRecoveryResponse {
    pub snapshot: TimelineRecoverySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineReplay {
    pub events: Vec<TimelineEventEnvelope>,
    #[serde(default)]
    pub high_water_mark_event_id: Option<String>,
    #[ts(type = "number")]
    pub last_replay_seq: i64,
    pub cursor_status: TimelineCursorStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineCursorStatus {
    Current,
    FromBeginning,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct SubscribeTimelineResponse {
    pub subscription_id: String,
    pub session_id: String,
    #[serde(default)]
    pub high_water_mark_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct TimelineActiveTurnSnapshot {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    pub status: TimelineActiveTurnStatus,
    #[serde(default)]
    pub last_event_id: Option<String>,
    #[serde(default)]
    pub pending_user_action: Option<TimelinePendingUserAction>,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineActiveTurnStatus {
    Running,
    WaitingUserAction,
    Cancelling,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TimelinePendingUserAction {
    DynamicToolResult(TimelinePendingDynamicToolResult),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct TimelinePendingDynamicToolResult {
    #[serde(default)]
    #[ts(optional)]
    pub item_id: Option<String>,
    pub call_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnCommandStatus {
    Pending,
    Accepted,
    Failed,
    FailedNoFirstEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct StartTurnResponse {
    pub request_id: String,
    #[serde(default)]
    pub message_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub status: TurnCommandStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct RetryTurnResponse {
    pub request_id: String,
    #[serde(default)]
    pub message_id: Option<String>,
    pub session_id: String,
    pub turn_id: String,
    pub status: TurnCommandStatus,
}

impl From<StartTurnResponse> for RetryTurnResponse {
    fn from(response: StartTurnResponse) -> Self {
        Self {
            request_id: response.request_id,
            message_id: response.message_id,
            session_id: response.session_id,
            turn_id: response.turn_id,
            status: response.status,
        }
    }
}

impl From<RetryTurnResponse> for StartTurnResponse {
    fn from(response: RetryTurnResponse) -> Self {
        Self {
            request_id: response.request_id,
            message_id: response.message_id,
            session_id: response.session_id,
            turn_id: response.turn_id,
            status: response.status,
        }
    }
}
