//! Transactional synchronization at the existing management database boundary.

mod facade;
mod metrics;
pub use metrics::read_account_metrics;
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
