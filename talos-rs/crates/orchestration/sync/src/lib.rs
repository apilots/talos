//! Transactional synchronization at the existing management database boundary.

mod facade;
mod projection;
mod runner;
mod store;
mod types;

pub use facade::TalosSync;
pub use runner::{SyncDataAdapter, SyncHandle};
pub use types::*;

#[cfg(test)]
#[path = "integration_tests.rs"]
mod tests;
