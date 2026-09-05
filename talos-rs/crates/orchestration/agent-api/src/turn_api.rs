use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_types::{AgentTurnRef, UserActionResponse};
use ts_rs::TS;

use crate::AgentUserInput;

/// Typed input accepted by one exact nonterminal Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase")]
pub enum SubmitTurnInput {
    /// Submit new User-authored content directly to a running Turn.
    Message { input: AgentUserInput },
    /// Atomically consume one queued message and deliver it to a running Turn.
    QueuedMessage { message_id: String },
    /// Answer the exact pending UserAction of a waiting Turn.
    UserActionResponse {
        action_id: String,
        response: UserActionResponse,
    },
}

/// Public request for delivering input to one exact nonterminal Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SubmitTurnInputRequest {
    pub identity: AgentTurnRef,
    pub input: SubmitTurnInput,
    #[ts(type = "string | null")]
    pub client_request_id: Option<String>,
}

/// Durable acknowledgement returned after Turn input is accepted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct SubmitTurnInputResponse {
    pub request_id: String,
    pub identity: AgentTurnRef,
    pub message_id: String,
}

/// Public request for interrupting one exact Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct InterruptTurnRequest {
    pub identity: AgentTurnRef,
}

/// Observable result of exact Turn cancellation and cleanup settlement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(rename_all = "camelCase")]
pub enum AgentInterruptOutcome {
    Cancelled,
    Completed,
    CleanupUnconfirmed { task_count: u32 },
}

/// Public response for one exact Turn interruption request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct InterruptTurnResponse {
    pub identity: AgentTurnRef,
    pub outcome: AgentInterruptOutcome,
}

#[cfg(test)]
#[path = "turn_api_tests.rs"]
mod tests;
