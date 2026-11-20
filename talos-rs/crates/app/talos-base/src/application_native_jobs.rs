//! Real host and Computer authorization for maintenance callbacks.
use std::sync::{Arc, Weak};

use talos_automation::{Automation, AutomationPauseUpdate};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{
    Actor, NativeJobFuture, NativeJobHost, NativeJobPauseUpdate, NativeJobRuntime,
    ResolvedNativeJobs,
};
use talos_storage::Storage;
use talos_types::{BuiltinJobAction, BuiltinJobId, BuiltinJobInfo, BuiltinJobOutcome};

use super::application_computer_maintenance::ComputerMaintenance;
use crate::application_sync::ApplicationSyncHost;

pub(crate) struct ApplicationNativeJobHost {
    pub sync_host: Weak<ApplicationSyncHost>,
    pub storage: Arc<dyn Storage>,
    pub automation: Automation,
    pub execution_instance_id: String,
    pub personal_desktop: bool,
    pub computer_maintenance: Option<Arc<ComputerMaintenance>>,
}

impl NativeJobHost for ApplicationNativeJobHost {
    fn resolve_owned<'a>(
        &'a self,
        actor: &'a Actor,
        requested_computer: &'a str,
    ) -> NativeJobFuture<'a, ResolvedNativeJobs> {
        Box::pin(async move {
            let host = self.sync_host.upgrade().ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Native job host is unavailable",
                )
            })?;
            if let Some(computers) = &self.computer_maintenance {
                if actor.auth_disabled {
                    return Err(scope_denied());
                }
                let computer_id = if requested_computer == "embedded" {
                    host.local_computer_id().await?
                } else {
                    requested_computer.to_owned()
                };
                let target = computers
                    .owned_target(&computer_id, &actor.user_id)
                    .await?
                    .ok_or_else(scope_denied)?;
                host.service
                    .nodes
                    .read_computer(
                        actor,
                        talos_types::GetNodeRequest {
                            node_id: computer_id.clone(),
                        },
                    )
                    .await?;
                computers.ensure(&target).await?;
                return Ok(self.resolve_runtime(
                    computer_id,
                    target.execution_instance_id.clone(),
                    target.job_prefix(),
                ));
            }
            if !actor.auth_disabled && !host.owns_local_computer(actor).await? {
                return Err(scope_denied());
            }
            let computer_id = host
                .local_computer_for_client(actor)
                .await?
                .ok_or_else(scope_denied)?;
            if computer_id != requested_computer || actor.user_id.is_empty() {
                return Err(scope_denied());
            }
            // The verified owner of a personal Desktop controls its local data,
            // including historical sessions under other identities.
            if !self.personal_desktop || actor.auth_disabled {
                verify_maintained_data_owner(self.storage.as_ref(), actor).await?;
            }
            Ok(self.resolve_runtime(
                computer_id,
                self.execution_instance_id.clone(),
                String::new(),
            ))
        })
    }
}

impl ApplicationNativeJobHost {
    fn resolve_runtime(
        &self,
        computer_id: String,
        execution_instance_id: String,
        job_prefix: String,
    ) -> ResolvedNativeJobs {
        let runtime = Arc::new(ApplicationNativeJobRuntime {
            automation: self.automation.clone(),
            computer_id: computer_id.clone(),
            execution_instance_id: execution_instance_id.clone(),
            job_prefix,
        });
        ResolvedNativeJobs {
            computer_id,
            execution_instance_id,
            runtime,
        }
    }
}

/// Node ownership is insufficient for callbacks that scan a whole database.
/// Inspect both actual owner columns, including usage without a surviving Session.
/// This is a current-data authorization check, not a permanent tenant lease.
async fn verify_maintained_data_owner(storage: &dyn Storage, actor: &Actor) -> TalosResult<()> {
    let predicate = if actor.auth_disabled {
        "user_id NOT IN (?, 'anonymous', 'embedded')"
    } else {
        "user_id <> ?"
    };
    let query = format!(
        "SELECT 1 AS foreign_owner FROM sessions WHERE user_id IS NULL OR {predicate} \
         UNION ALL SELECT 1 FROM profile_turn_usage WHERE user_id IS NULL OR {predicate} LIMIT 1"
    );
    if !storage
        .query(&query, &[&actor.user_id, &actor.user_id])
        .await?
        .is_empty()
    {
        return Err(scope_denied());
    }
    Ok(())
}

fn scope_denied() -> TalosError {
    TalosError::new(ErrorCode::PermissionDenied, "Native job controls require ownership of the actual execution instance and its maintained data")
}

struct ApplicationNativeJobRuntime {
    automation: Automation,
    computer_id: String,
    execution_instance_id: String,
    job_prefix: String,
}

impl NativeJobRuntime for ApplicationNativeJobRuntime {
    fn list(&self) -> NativeJobFuture<'_, Vec<BuiltinJobInfo>> {
        Box::pin(async move {
            self.automation
                .jobs()
                .await?
                .into_iter()
                .filter_map(|job| {
                    let name = job.name.strip_prefix(&self.job_prefix)?.to_owned();
                    Some((name, job))
                })
                .map(|(name, job)| {
                    let id = match name.as_str() {
                        "profile-usage-retention" => BuiltinJobId::ProfileUsageRetention,
                        "dream-memory-maintenance" => BuiltinJobId::DreamMemoryMaintenance,
                        _ => {
                            return Err(TalosError::internal(
                                "A managed native job has no public identity",
                            ))
                        }
                    };
                    let mut available_actions = vec![if job.paused {
                        BuiltinJobAction::Resume
                    } else {
                        BuiltinJobAction::Pause
                    }];
                    if job.run_once && !job.running {
                        available_actions.push(BuiltinJobAction::RunOnce);
                    }
                    let last_outcome = job.last_finished_at.map(|_| match job.last_error {
                        Some(error) => BuiltinJobOutcome::Failed { error },
                        None => BuiltinJobOutcome::Succeeded,
                    });
                    Ok(BuiltinJobInfo {
                        id,
                        name,
                        computer_id: self.computer_id.clone(),
                        execution_instance_id: self.execution_instance_id.clone(),
                        schedule: job.schedule,
                        timezone: job.timezone.to_string(),
                        paused: job.paused,
                        running: job.running,
                        next_due_at: job.next_due_at.map(|at| at.timestamp()),
                        last_finished_at: job.last_finished_at.map(|at| at.timestamp()),
                        last_outcome,
                        available_actions,
                    })
                })
                .collect()
        })
    }

    fn set_paused(&self, update: NativeJobPauseUpdate) -> NativeJobFuture<'_, ()> {
        Box::pin(async move {
            let name = format!("{}{}", self.job_prefix, update.job_id.as_str());
            self.automation
                .set_paused(AutomationPauseUpdate {
                    name: &name,
                    paused: update.paused,
                })
                .await
        })
    }

    fn run_once(&self, job_id: BuiltinJobId) -> NativeJobFuture<'_, ()> {
        Box::pin(async move {
            let name = format!("{}{}", self.job_prefix, job_id.as_str());
            self.automation.run_once(&name).await
        })
    }
}

#[cfg(test)]
#[path = "application_native_jobs_tests.rs"]
mod tests;
