use serde::{Deserialize, Serialize};

use crate::{AgentMemoryModelUsage, AgentSessionContext};

/// Position in the Worker's retained Dream execution facts for one Session generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DreamMemoryCursor {
    pub generation: u64,
    pub offset: i64,
}

/// Form one bounded batch or replay the next retained formation after this product cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct DreamMemoryParams {
    pub session: AgentSessionContext,
    pub after_cursor: Option<DreamMemoryCursor>,
}

/// Worker execution facts retained independently of product accounting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DreamMemoryFormation {
    pub covers_until_seq: Option<u64>,
    pub model_usage: AgentMemoryModelUsage,
    pub failure: Option<talos_error::ApiError>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DreamMemoryResult {
    pub formation: Option<DreamMemoryFormation>,
    pub next_cursor: DreamMemoryCursor,
    /// Current Worker progress; safe to consume only after all returned formation facts.
    pub processed_until_seq: u64,
}
