use serde::{Deserialize, Serialize};

/// Opaque lookup key for Provider-owned durable continuation state.
///
/// Service persists this value but must never parse or reinterpret it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentContinuationRef {
    pub opaque_id: String,
}
