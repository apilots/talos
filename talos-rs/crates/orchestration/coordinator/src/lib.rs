mod coordinator;
mod r#trait;
mod types;

pub use coordinator::CoordinatorImpl;
pub use r#trait::Coordinator;
pub type RawAgentEvent = talos_agent::OrchestratorEvent;
pub use types::{CoordinatorResponse, CoordinatorStatus};

// Re-export commonly used items
pub mod prelude {
    pub use super::{
        Coordinator, CoordinatorImpl, CoordinatorResponse, CoordinatorStatus, RawAgentEvent,
    };
}
