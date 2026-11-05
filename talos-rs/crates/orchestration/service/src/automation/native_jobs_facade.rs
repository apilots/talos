//! Public management remains on the original Service/SDK path, never model Tools.
use super::TalosService;
use crate::{automation::native_jobs::NativeJobService, Actor};
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    BuiltinJobControlRequest, BuiltinJobInfo, BuiltinJobListRequest, BuiltinJobListResponse,
};

impl TalosService {
    pub fn install_native_job_control(&self, service: Arc<NativeJobService>) -> TalosResult<()> {
        self.native_jobs.set(service).map_err(|_| {
            TalosError::new(
                ErrorCode::Conflict,
                "Native job control is already installed",
            )
        })
    }

    fn native_job_control(&self) -> TalosResult<&Arc<NativeJobService>> {
        self.native_jobs.get().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Native job control is unavailable on this host",
            )
        })
    }

    pub async fn list_builtin_jobs(
        &self,
        actor: &Actor,
        request: BuiltinJobListRequest,
    ) -> TalosResult<BuiltinJobListResponse> {
        self.native_job_control()?.list(actor, request).await
    }

    pub async fn control_builtin_job(
        &self,
        actor: &Actor,
        request: BuiltinJobControlRequest,
    ) -> TalosResult<BuiltinJobInfo> {
        self.native_job_control()?.control(actor, request).await
    }
}
