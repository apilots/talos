use std::sync::Arc;

use talos_error::TalosResult;
use talos_service::TalosService;
use tracing::info;

pub struct ServiceGateway {
    pub(crate) service: Arc<TalosService>,
}

impl ServiceGateway {
    pub fn new(service: Arc<TalosService>) -> Self {
        Self { service }
    }

    pub(crate) async fn start(&self) -> TalosResult<()> {
        info!("Starting gateway...");
        Ok(())
    }
}

pub(crate) fn require_non_empty(value: &str, field: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        return Err(talos_error::TalosError::ValidationError(format!(
            "{field} is required"
        )));
    }
    Ok(())
}
