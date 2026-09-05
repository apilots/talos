use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AgentRunResult, AgentTurnRef};

/// One terminal Turn in canonical Session history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionHistoryTurn {
    pub identity: AgentTurnRef,
    pub result: AgentRunResult,
    #[ts(type = "number")]
    pub completed_event_seq: i64,
    #[ts(type = "number")]
    pub completed_at: i64,
}

/// Bounded canonical Session history shared by every transport and SDK.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionHistoryResponse {
    pub data: Vec<SessionHistoryTurn>,
    pub next_cursor: Option<String>,
}
