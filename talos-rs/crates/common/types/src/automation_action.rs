//! Closed Goal/Loop operations. Native maintenance jobs are a separate API.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AgentCaller, AutomationTurnOrigin, LoopSchedule, SessionAutomationSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(
    tag = "target",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationAction {
    Goal { action: GoalAction },
    Loop { action: LoopAction },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GoalAction {
    Create {
        objective: String,
        #[ts(type = "number | null")]
        #[serde(rename = "tokenBudget")]
        token_budget: Option<u64>,
    },
    Read {},
    Edit {
        #[serde(rename = "goalId")]
        goal_id: String,
        #[ts(type = "number")]
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
        objective: String,
    },
    Pause {
        #[serde(rename = "goalId")]
        goal_id: String,
    },
    Resume {
        #[serde(rename = "goalId")]
        goal_id: String,
    },
    Clear {
        #[serde(rename = "goalId")]
        goal_id: String,
    },
    /// Model-only declaration; effective only after matching normal settlement.
    Complete {
        #[serde(rename = "goalId")]
        goal_id: String,
        #[ts(type = "number")]
        #[serde(rename = "objectiveRevision")]
        objective_revision: u64,
        evidence: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LoopAction {
    Create {
        prompt: String,
        schedule: LoopSchedule,
    },
    List {},
    Edit {
        #[serde(rename = "loopId")]
        loop_id: String,
        #[ts(type = "number")]
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
        prompt: String,
        schedule: LoopSchedule,
    },
    Stop {
        #[serde(rename = "loopId")]
        loop_id: String,
    },
    Clear {
        #[serde(rename = "loopId")]
        loop_id: String,
    },
    /// Model-only decision; effective only after matching normal settlement.
    Decide {
        #[serde(rename = "loopId")]
        loop_id: String,
        #[ts(type = "number")]
        #[serde(rename = "configRevision")]
        config_revision: u64,
        decision: LoopWakeDecision,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LoopWakeDecision {
    Continue {
        #[serde(rename = "delayMinutes")]
        #[schemars(range(min = 1))]
        delay_minutes: u32,
        reason: String,
    },
    Stop {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRequest {
    pub session_id: String,
    pub action: AutomationAction,
}

/// Internal Tool/RPC envelope: caller is supplied by authenticated invocation,
/// never by the model's tool arguments. Services revalidate its durable Turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationModelRequest {
    pub caller: AgentCaller,
    /// Runtime Tool call identity, never accepted from model arguments.
    pub call_id: String,
    pub action: AutomationAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AutomationEffect {
    Applied,
    PendingSettlement,
}

/// Exact object identity issued by the creation owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationObjectRef {
    Goal {
        #[serde(rename = "goalId")]
        goal_id: String,
    },
    Loop {
        #[serde(rename = "loopId")]
        loop_id: String,
    },
}

/// Declarations are acknowledged without claiming their terminal effect happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationControlResult {
    pub effect: AutomationEffect,
    /// Create operations return the exact object, including idempotent replies.
    /// Never infer a new Loop's identity from prompt text or snapshot differences.
    pub created_object: Option<AutomationObjectRef>,
    /// Exact invocation binding for the model; multiple Loops may have the same
    /// prompt. Management calls without an invoking Turn return null.
    pub bound_origin: Option<AutomationTurnOrigin>,
    pub snapshot: SessionAutomationSnapshot,
}
