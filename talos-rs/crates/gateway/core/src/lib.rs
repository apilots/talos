mod automation;
mod computers;
mod config;
mod file;
mod fs;
pub mod gateway;
pub mod gateway_service;
mod memory;
mod message_queue;
mod model;
mod plugin;
mod profile;
mod project;

mod session;
mod skills;
mod turn;
mod user_auth;
mod workspace;

pub use gateway::Gateway;

use std::sync::Arc;
use talos_error::TalosResult;
use talos_service::TalosService;

pub struct ServiceGateway {
    pub(crate) service: Arc<TalosService>,
}

impl ServiceGateway {
    pub fn new(service: Arc<TalosService>) -> Self {
        Self { service }
    }

    pub(crate) async fn start(&self) -> TalosResult<()> {
        tracing::info!("Starting gateway...");
        Ok(())
    }
}

pub(crate) fn require_non_empty(value: &str, field: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        return Err(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("{field} is required"),
        ));
    }
    Ok(())
}
