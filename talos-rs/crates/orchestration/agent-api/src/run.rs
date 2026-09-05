use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AgentMessage, AgentRunMetrics, AgentTurnCompleted, AgentTurnFailed};

/// Canonical successful Turn result derived by Harness from one reducer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct AgentRunOutput {
    pub messages: Vec<AgentMessage>,
    pub metrics: AgentRunMetrics,
    pub completion: AgentTurnCompleted,
}

/// Canonical failed Turn result preserving visible partial output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
pub struct AgentRunFailure {
    pub messages: Vec<AgentMessage>,
    pub metrics: AgentRunMetrics,
    pub failure: AgentTurnFailed,
}

/// Unique completed or failed result of one logical Agent Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(tag = "status", content = "result", rename_all = "camelCase")]
pub enum AgentRunResult {
    Completed(AgentRunOutput),
    Failed(AgentRunFailure),
}
