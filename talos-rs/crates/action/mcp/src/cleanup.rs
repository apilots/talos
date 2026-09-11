use std::sync::Mutex;

use talos_error::{TalosError, TalosResult};
use tokio_util::task::TaskTracker;

/// Tracks cleanup even when a connection attempt is cancelled before publication.
/// The first failure is retained so explicit application shutdown cannot claim
/// success after a detached process or transport failed to close.
#[derive(Default)]
pub(super) struct Cleanup {
    pub tasks: TaskTracker,
    error: Mutex<Option<TalosError>>,
}

impl Cleanup {
    pub fn record(&self, error: TalosError) {
        let mut first = self.error.lock().expect("MCP cleanup lock poisoned");
        if first.is_none() {
            *first = Some(error);
        } else {
            // Shutdown returns the first failure; only secondary errors are reported here.
            talos_logging::report_error(&error);
        }
    }

    pub fn result(&self) -> TalosResult<()> {
        match self
            .error
            .lock()
            .expect("MCP cleanup lock poisoned")
            .as_ref()
        {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}
