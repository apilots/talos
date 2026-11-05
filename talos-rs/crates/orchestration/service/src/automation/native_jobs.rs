//! Owner-authorized business control of the one original native Job runtime.
//! Application composition supplies scope/owner resolution; no app dependency exists here.
use crate::{
    native_jobs::{NativeJobHost, NativeJobPauseUpdate, ResolvedNativeJobs},
    Actor,
};
use std::sync::{Arc, Weak};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::automation::native_jobs::{NativeJobControlStore, SetNativeJobPaused};
use talos_types::{
    BuiltinJobAction, BuiltinJobControlRequest, BuiltinJobInfo, BuiltinJobListRequest,
    BuiltinJobListResponse,
};
use tokio::sync::Mutex;

/// Application runtime keeps the host alive. Weak ownership breaks the cycle
/// through Dream callbacks that already retain TalosService.
pub struct NativeJobService {
    controls: Arc<dyn NativeJobControlStore>,
    host: Weak<dyn NativeJobHost>,
    operations: Mutex<()>,
}

impl NativeJobService {
    pub fn new(controls: Arc<dyn NativeJobControlStore>, host: Weak<dyn NativeJobHost>) -> Self {
        Self {
            controls,
            host,
            operations: Mutex::new(()),
        }
    }

    async fn resolve(&self, actor: &Actor, computer_id: &str) -> TalosResult<ResolvedNativeJobs> {
        let host = self.host.upgrade().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Native job management is unavailable.",
            )
        })?;
        let resolved = host.resolve_owned(actor, computer_id).await?;
        if resolved.computer_id.is_empty() || resolved.execution_instance_id.is_empty() {
            return Err(TalosError::internal(
                "Native job host returned an invalid execution scope.",
            ));
        }
        Ok(resolved)
    }

    pub async fn list(
        &self,
        actor: &Actor,
        request: BuiltinJobListRequest,
    ) -> TalosResult<BuiltinJobListResponse> {
        let scope = self.resolve(actor, &request.computer_id).await?;
        let _operation = self.operations.lock().await;
        Ok(BuiltinJobListResponse {
            data: snapshots(&scope).await?,
        })
    }

    /// Pause/resume persists before runtime application; a failed application
    /// remains an explicit error, with durable control retained for next startup.
    /// Run-once is admission only, never an implicit resume or callback duplicate.
    pub async fn control(
        &self,
        actor: &Actor,
        request: BuiltinJobControlRequest,
    ) -> TalosResult<BuiltinJobInfo> {
        let scope = self.resolve(actor, &request.computer_id).await?;
        if scope.execution_instance_id != request.execution_instance_id {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "The native job execution instance has changed.",
            ));
        }
        let _operation = self.operations.lock().await;
        let before = snapshots(&scope)
            .await?
            .into_iter()
            .find(|job| job.id == request.job_id)
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ResourceNotFound,
                    "The native job is unavailable on this execution instance.",
                )
            })?;
        if request.action == BuiltinJobAction::RunOnce && before.running {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "The native job is already running.",
            ));
        }
        let unchanged = matches!(request.action, BuiltinJobAction::Pause) && before.paused
            || matches!(request.action, BuiltinJobAction::Resume) && !before.paused;
        if !unchanged && !before.available_actions.contains(&request.action) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "This action is unavailable for the native job.",
            ));
        }
        match request.action {
            BuiltinJobAction::Pause | BuiltinJobAction::Resume => {
                let paused = request.action == BuiltinJobAction::Pause;
                self.controls
                    .set_paused(SetNativeJobPaused {
                        execution_instance_id: &scope.execution_instance_id,
                        job_id: request.job_id,
                        paused,
                    })
                    .await?;
                scope
                    .runtime
                    .set_paused(NativeJobPauseUpdate {
                        job_id: request.job_id,
                        paused,
                    })
                    .await?;
            }
            BuiltinJobAction::RunOnce => scope.runtime.run_once(request.job_id).await?,
        }
        snapshots(&scope)
            .await?
            .into_iter()
            .find(|job| job.id == request.job_id)
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ResourceNotFound,
                    "The native job is no longer available.",
                )
            })
    }
}

async fn snapshots(scope: &ResolvedNativeJobs) -> TalosResult<Vec<BuiltinJobInfo>> {
    let jobs = scope.runtime.list().await?;
    for (index, job) in jobs.iter().enumerate() {
        if job.computer_id != scope.computer_id
            || job.execution_instance_id != scope.execution_instance_id
            || jobs[..index].iter().any(|previous| previous.id == job.id)
        {
            return Err(TalosError::internal(
                "Native job runtime returned inconsistent execution scope.",
            ));
        }
    }
    Ok(jobs)
}

#[cfg(test)]
#[path = "native_jobs_tests.rs"]
mod tests;
