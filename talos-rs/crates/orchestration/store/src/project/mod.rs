mod mock_store;
mod store;
mod types;

pub use mock_store::MockProjectStore;
pub use store::{ProjectStore, StorageProjectStore};
pub use types::ProjectRecord;

#[cfg(test)]
#[path = "affinity_tests.rs"]
mod affinity_tests;
