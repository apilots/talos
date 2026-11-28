use talos_agent_api::{AgentMessage, AgentRunMetrics};

/// Durable reducer state reconstructed from canonical Agent events.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentRunCheckpoint {
    pub last_sequence: u64,
    pub messages: Vec<AgentMessage>,
    pub metrics: AgentRunMetrics,
}
