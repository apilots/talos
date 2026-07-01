mod coordinator;
mod registry;
mod session_runtime;
mod r#trait;

pub use coordinator::CoordinatorImpl;
pub use r#trait::Coordinator;
pub use registry::{AgentRegistration, AgentRegistry};
pub use session_runtime::SessionRuntime;

// Re-export commonly used items
pub mod prelude {
    pub use super::{Coordinator, CoordinatorImpl, SessionRuntime};
}
