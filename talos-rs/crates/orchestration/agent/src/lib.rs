mod agent;
mod service;
mod r#trait;
mod types;

pub use agent::AgentImpl as LLMAgent;
pub use agent::{AgentImpl, MockAgent};
pub use r#trait::Agent;
pub use service::{
    AgentOrchestrator, AgentRequest, OrchestratorEvent, OrchestratorExecution, OrchestratorRequest,
};
pub use talos_agent_kernel::TurnPhase;
pub use types::{AgentResponse, AgentStream, ToolCall};

// Re-export commonly used types
