//! Worker-owned title proposal for one committed successful Turn.
use crate::{AgentMemoryModelUsage, AgentSessionContext};
use serde::{Deserialize, Serialize};
use talos_agent_api::AgentTurnRef;
use talos_error::ApiError;

/// The Worker reads local history and uses its configured built-in model.
/// Callers supply trusted Session scope and exclude concurrent Session maintenance.
#[derive(Debug, Clone)]
pub struct AgentSessionTitleParams {
    pub session: AgentSessionContext,
    pub mode: AgentSessionTitleMode,
    pub identity: AgentTurnRef,
}

/// Existing product titles suppress new calls while retained usage stays consumable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSessionTitleMode {
    Generate,
    ReadRetained,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentSessionTitleOutcome {
    Generated { title: String },
    Failed { error: ApiError },
}

/// Retained execution fact; the product owns title persistence and idempotent accounting.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionTitleResult {
    pub outcome: AgentSessionTitleOutcome,
    pub model_usage: Option<AgentMemoryModelUsage>,
}
