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

/// Compare the logical vectors of two persisted cuts.
pub fn same_version(left: &SyncVersion, right: &SyncVersion) -> talos_error::TalosResult<bool> {
    talos_sync_sdk::version_equal(left, right).map_err(facade::sync_error)
}

#[cfg(test)]
#[path = "integration_tests.rs"]
mod tests;
