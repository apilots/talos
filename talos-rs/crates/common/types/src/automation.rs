//! Session-owned automation facts shared by Services and clients.
//!
//! Definitions are durable. Queue occurrences and reservations remain owned by
//! the existing in-memory Session queue; these types do not add a run ledger.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Initial cancellation window before the first admitted Goal/Loop work.
pub const AUTOMATION_INITIAL_DELAY_SECONDS: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AutomationMode {
    Goal,
    Loop,
}

impl AutomationMode {
    pub const fn skill_name(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Loop => "loop",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LoopSchedule {
    FixedInterval {
        #[schemars(range(min = 1))]
        minutes: u32,
    },
    ModelDecided {},
}

impl LoopSchedule {
    pub fn validate(&self) -> crate::TalosResult<()> {
        if matches!(self, Self::FixedInterval { minutes: 0 }) {
            return Err(crate::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Loop interval must be at least one minute",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum GoalStatus {
    Active,
    Paused,
    Complete,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum GoalPauseReason {
    UserPaused,
    UserCancelled,
    WaitingForUser,
    ExecutionFailed,
    BudgetExhausted,
    NoProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum LoopStatus {
    Active,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum LoopStopReason {
    UserStopped,
    UserCancelled,
    ModelStopped,
    WaitingForUser,
    ExecutionFailed,
    MissingDecision,
    QueueFull,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionGoal {
    pub id: String,
    pub session_id: String,
    pub objective: String,
    #[ts(type = "number")]
    pub objective_revision: u64,
    pub status: GoalStatus,
    pub pause_reason: Option<GoalPauseReason>,
    pub reason: Option<String>,
    #[ts(type = "number | null")]
    pub token_budget: Option<u64>,
    #[ts(type = "number")]
    pub tokens_used: u64,
    pub latest_turn_id: Option<String>,
    pub completion_evidence: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionLoop {
    pub id: String,
    pub session_id: String,
    pub prompt: String,
    pub schedule: LoopSchedule,
    #[ts(type = "number")]
    pub config_revision: u64,
    pub status: LoopStatus,
    pub stop_reason: Option<LoopStopReason>,
    pub reason: Option<String>,
    #[ts(type = "number | null")]
    pub next_due_at: Option<i64>,
    pub latest_turn_id: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// Provenance bound at admission (or to the explicit initial Goal Turn).
/// It is never inferred from text or the Session's current mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationTurnOrigin {
    Goal {
        #[serde(rename = "goalId")]
        goal_id: String,
        #[ts(type = "number")]
        #[serde(rename = "objectiveRevision")]
        objective_revision: u64,
    },
    Loop {
        #[serde(rename = "loopId")]
        loop_id: String,
        #[serde(rename = "occurrenceId")]
        occurrence_id: String,
        #[ts(type = "number")]
        #[serde(rename = "configRevision")]
        config_revision: u64,
    },
}

/// Exact triggering Input message. Other human/Steer messages in the same Turn
/// keep ordinary provenance, even while that Turn belongs to Goal or Loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationInputOrigin {
    pub message_id: String,
    pub origin: AutomationTurnOrigin,
}

/// Queue provenance is process-local, even when its definition is durable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum QueuedMessageSource {
    #[default]
    User,
    Loop {
        #[serde(rename = "loopId")]
        loop_id: String,
        #[serde(rename = "occurrenceId")]
        occurrence_id: String,
    },
}

/// Current Session facts. Historical definitions use the paginated list API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionAutomationSnapshot {
    pub session_id: String,
    pub goal: Option<SessionGoal>,
    pub loops: Vec<SessionLoop>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionAutomationReadRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationListEntry {
    Goal {
        goal: SessionGoal,
    },
    Loop {
        #[serde(rename = "loopTask")]
        loop_task: SessionLoop,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AutomationListScope {
    Unfinished,
    Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationListRequest {
    /// Omitted/null lists the authenticated user's accessible Sessions.
    pub session_id: Option<String>,
    pub scope: AutomationListScope,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationListResponse {
    pub data: Vec<AutomationListEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationChangedNotification {
    pub session_id: String,
}
