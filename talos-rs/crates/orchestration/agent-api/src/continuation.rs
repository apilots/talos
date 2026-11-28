use serde::{Deserialize, Serialize};

/// Opaque lookup key for Provider-owned durable continuation state.
///
/// Only the Worker persists and resolves this value; product services and transports never hold it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentContinuationRef {
    pub opaque_id: String,
}
