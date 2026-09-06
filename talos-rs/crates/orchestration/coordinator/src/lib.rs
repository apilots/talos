mod agent_coordinator;
mod registry;
mod router;
mod r#trait;

#[cfg(test)]
mod test_support;

pub use agent_coordinator::CoordinatorImpl;
pub use r#trait::AgentCoordinator;
pub(crate) use registry::AgentRegistry;
pub(crate) use router::AgentRouter;
pub use router::AgentSelectionParams;

// Re-export commonly used items
pub mod prelude {
    pub use super::{AgentCoordinator, CoordinatorImpl};
}
