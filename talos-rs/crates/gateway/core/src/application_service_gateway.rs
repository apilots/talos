use std::sync::Arc;

use talos_application_service::TalosApplicationService;
use talos_error::TalosResult;
use tracing::info;

pub struct ApplicationServiceGateway {
    pub(crate) application_service: Arc<TalosApplicationService>,
}

impl ApplicationServiceGateway {
    pub fn new(application_service: Arc<TalosApplicationService>) -> Self {
        Self {
            application_service,
        }
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
