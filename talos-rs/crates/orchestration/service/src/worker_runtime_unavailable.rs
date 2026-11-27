//! Server-only runtime management is absent in personal products.
use crate::{Actor, TalosService};
use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{WorkerRuntimeCandidate, WorkerRuntimeSettingsResponse};
impl TalosService {
    pub fn read_worker_runtime_settings(
        &self,
        _actor: &Actor,
        _active: &Config,
    ) -> TalosResult<WorkerRuntimeSettingsResponse> {
        Err(unavailable())
    }
    pub async fn validate_worker_runtime_settings(
        &self,
        _actor: &Actor,
        _active: &Config,
        _request: WorkerRuntimeCandidate,
    ) -> TalosResult<()> {
        Err(unavailable())
    }
    pub async fn update_worker_runtime_settings(
        &self,
        _actor: &Actor,
        _active: &Config,
        _request: WorkerRuntimeCandidate,
    ) -> TalosResult<WorkerRuntimeSettingsResponse> {
        Err(unavailable())
    }
}
fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Worker runtime management requires the Talos Server control plane.",
    )
}
