use serde::{Deserialize, Serialize};
use talos_agent_api::{AgentEvent, AgentRunMetrics, AgentTurnRef};

use crate::AgentSessionContext;

/// Exclusive position in one Worker's durable Session generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionEventCursor {
    pub session_id: String,
    pub generation: u64,
    pub offset: i64,
}

/// Durable settlement facts needed to recover a missed live delivery.
/// Original in-process errors remain on the live settlement; replay uses the
/// canonical terminal event and never reconstructs diagnostics from JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum AgentExecutionSettlement {
    WaitingForInput {
        metrics: AgentRunMetrics,
    },
    Finished {
        memory_usage: Vec<AgentMemoryModelUsage>,
        metrics: AgentRunMetrics,
    },
}

/// Durable operation kind selected by the original Worker admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentExecutionPurpose {
    Turn,
    Compaction,
}

/// One committed execution fact, independent of product Timeline notices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentCommittedEvent {
    pub execution_purpose: AgentExecutionPurpose,
    pub history_supplement: Option<talos_agent_api::trajectory::SessionHistorySupplement>,
    pub execution_model: Option<talos_types::ModelRef>,
    pub cursor: AgentSessionEventCursor,
    pub event: AgentEvent,
    pub settlement: Option<AgentExecutionSettlement>,
    pub automation_input: Option<talos_types::AutomationInputOrigin>,
    pub command: Option<crate::AgentCommandIdentity>,
}

/// Authorized, bounded replay of a single Session's execution facts.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSessionEventsParams {
    pub session: AgentSessionContext,
    pub after_cursor: Option<AgentSessionEventCursor>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionEventsResult {
    pub events: Vec<AgentCommittedEvent>,
    pub next_cursor: AgentSessionEventCursor,
    pub has_more: bool,
    pub active_turn: Option<AgentTurnRef>,
}

pub const MAX_EXECUTION_CAPTURE_BYTES: usize = 256 * 1024;
pub const MAX_EXECUTION_EVENT_BYTES: usize = 512 * 1024;
pub const MAX_EXECUTION_PAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_EXECUTION_PAGE_EVENTS: usize = 256;

/// Complete typed input accepted by the execution owner, including the chosen
/// configuration and causal version. Used for durable idempotency comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum AgentExecutionCommand {
    Run {
        params: Box<crate::AgentRunParams>,
    },
    Input {
        params: Box<crate::AgentInputParams>,
    },
    Compaction {
        context: Box<crate::AgentContext>,
    },
}

/// Worker-local background Memory model usage, committed with its Turn settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMemoryModelUsage {
    pub request_id: String,
    pub model_ref: talos_types::ModelRef,
    pub usage: talos_types::TokenUsage,
    pub completed_at: i64,
    pub timezone: String,
}

/// Read one sealed Worker-owned trajectory body in bounded UTF-8 chunks.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentExecutionPayloadParams {
    pub session: AgentSessionContext,
    pub identity: AgentTurnRef,
    pub payload_id: String,
    pub offset: u64,
    pub limit: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentExecutionPayloadResult {
    pub data: String,
    pub next_offset: Option<u64>,
}
