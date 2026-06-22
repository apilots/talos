mod coordinator;
mod r#trait;

pub use coordinator::CoordinatorImpl;
pub use r#trait::Coordinator;

// Re-export commonly used items
pub mod prelude {
    pub use super::{Coordinator, CoordinatorImpl};
}
