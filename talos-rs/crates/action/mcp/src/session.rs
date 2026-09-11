use std::sync::Arc;

use talos_error::{ErrorCode, TalosError};

use crate::{cleanup::Cleanup, process::McpProcess, protocol::RmcpService, CLEANUP_TIMEOUT};

/// Keeps SDK tasks owned while startup, discovery or subscription can still fail.
/// Once ready, the connection worker takes both handles and closes them explicitly.
pub(super) struct McpSession {
    service: Option<RmcpService>,
    pub process: Option<McpProcess>,
    cleanup: Arc<Cleanup>,
}

impl McpSession {
    pub fn new(service: RmcpService, cleanup: Arc<Cleanup>) -> Self {
        Self {
            service: Some(service),
            process: None,
            cleanup,
        }
    }

    pub fn into_parts(mut self) -> (RmcpService, Option<McpProcess>) {
        (
            self.service.take().expect("MCP session is owned"),
            self.process.take(),
        )
    }
}

impl std::ops::Deref for McpSession {
    type Target = RmcpService;
    fn deref(&self) -> &Self::Target {
        self.service.as_ref().expect("MCP session is owned")
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        if let Some(service) = self.service.take() {
            let cleanup = self.cleanup.clone();
            self.cleanup.tasks.spawn(async move {
                match tokio::time::timeout(CLEANUP_TIMEOUT, service.cancel()).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(error)) => cleanup.record(TalosError::from_source(
                        ErrorCode::RunCleanupUnconfirmed,
                        "MCP transport cleanup failed.",
                        error,
                    )),
                    Err(_) => cleanup.record(TalosError::new(
                        ErrorCode::RunCleanupUnconfirmed,
                        "MCP transport cleanup did not finish within the deadline.",
                    )),
                }
            });
        }
    }
}
