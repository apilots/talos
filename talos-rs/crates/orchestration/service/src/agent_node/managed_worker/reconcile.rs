//! Advance durable checkpoints and reconcile them with Docker and Worker registration.

use super::*;
use std::collections::HashSet;
use std::path::PathBuf;
use talos_config::AgentNodeRunnerConfig;
use talos_store::node::SetNodeStatusParams;
use talos_worker_runtime::{SeedFile, SeedMode};
use tokio_util::sync::CancellationToken;

const CREDENTIAL_LIFETIME_SECONDS: i64 = 365 * 24 * 60 * 60;
const WORKER_HOME: &str = "/home/talos/.talos";
const CREDENTIAL_PATH: &str = "conf/node.credential";
const CA_PATH: &str = "conf/control-plane-ca.pem";

impl AgentNodeService {
    /// Reconcile persisted intent after startup and after requests disconnect.
    pub async fn run_managed_workers(&self, shutdown: CancellationToken) {
        let Ok(managed) = self.managed_runtime() else {
            return;
        };
        let start = now();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut changed = managed.changed.subscribe();
        let mut active = HashSet::new();
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                job = jobs.join_next(), if !jobs.is_empty() => {
                    match job {
                        Some(Ok(id)) => { active.remove(&id); }
                        Some(Err(error)) => {
                            talos_logging::report_error(&TalosError::internal_with_source(
                                "Talo reconciliation task failed.", error,
                            ));
                            break;
                        }
                        None => {}
                    }
                    continue;
                }
                _ = tick.tick() => {},
                _ = changed.changed() => {},
            }
            let idle_timeout = i64::from(managed.policy.idle_timeout_seconds);
            let idle_before = if now() - start >= idle_timeout && now() % 15 == 0 {
                now() - idle_timeout
            } else {
                0
            };
            match self
                .store
                .managed_workers_needing_attention(idle_before)
                .await
            {
                Ok(records) => {
                    for record in records {
                        if active.insert(record.instance_id.clone()) {
                            let service = self.clone();
                            jobs.spawn(async move {
                                let id = record.instance_id.clone();
                                service.reconcile_managed_worker(record).await;
                                id
                            });
                        }
                    }
                }
                Err(error) => talos_logging::report_error(&error),
            }
        }
    }

    async fn reconcile_managed_worker(&self, mut record: ManagedWorkerRecord) {
        let result = if record.step == Step::Stable {
            self.sleep_managed_worker(&mut record).await
        } else {
            self.advance_managed_worker(&mut record).await
        };
        if let Err(error) = result {
            if record.step == Step::Stable {
                return;
            }
            if error.is_code(ErrorCode::Conflict)
                && self
                    .store
                    .managed_worker_by_node(&record.node_id)
                    .await
                    .ok()
                    .flatten()
                    .is_none_or(|latest| latest.generation != record.generation)
            {
                return;
            }
            talos_logging::report_error(&error);
            if record.operation == Some(Action::Wake) {
                self.recover_failed_wake(&mut record, &error).await;
            }
            record.error = Some(error.to_api_error().into());
            match self
                .store
                .write_managed_worker(&record, record.generation, Write::Checkpoint)
                .await
            {
                Err(error) if !error.is_code(ErrorCode::Conflict) => {
                    talos_logging::report_error(&error)
                }
                _ => {}
            }
            self.managed
                .get()
                .expect("installed managed runtime")
                .changed
                .send_replace(());
        }
    }

    async fn recover_failed_wake(&self, record: &mut ManagedWorkerRecord, error: &TalosError) {
        let (Ok(managed), Ok(spec)) = (self.managed_runtime(), runtime_spec(record)) else {
            return;
        };
        let Ok(observation) = managed.runtime.inspect(&spec).await else {
            return;
        };
        if observation.container == ContainerState::Running {
            // A late registration clears this failure and starts a new idle window.
            record.step = Step::Wait;
        } else if error.is_code(ErrorCode::ServiceUnavailable) || error.is_code(ErrorCode::Timeout)
        {
            record.desired = Desired::IdleSleep;
            record.step = Step::Stable;
            record.operation_at = now();
        }
    }

    async fn sleep_managed_worker(&self, record: &mut ManagedWorkerRecord) -> TalosResult<()> {
        let managed = self.managed_runtime()?;
        let _submit = self.managed_submission.lock().await;
        let _admission = self
            .admission
            .for_node(&record.node_id)
            .try_write_owned()
            .map_err(|_| busy())?;
        let Some(latest) = self.store.managed_worker_by_node(&record.node_id).await? else {
            return Ok(());
        };
        if latest.generation != record.generation
            || latest.last_activity_at > now() - i64::from(managed.policy.idle_timeout_seconds)
            || !self.managed_node_is_available(&record.node_id).await?
        {
            return Ok(());
        }
        let expected_generation = record.generation;
        update_intent(record, Action::Sleep)?;
        record.desired = Desired::IdleSleep;
        record.step = Step::Stop;
        self.suspend_managed_worker_with_admission(record, expected_generation)
            .await?;
        managed.changed.send_replace(());
        Ok(())
    }

    async fn advance_managed_worker(&self, record: &mut ManagedWorkerRecord) -> TalosResult<()> {
        let managed = self.managed_runtime()?;
        let spec = runtime_spec(record)?;
        let runtime = &managed.runtime;
        let mut committed = record.clone();
        let next = match record.step {
            Step::Provision => {
                let path_in_home = |path| PathBuf::from(WORKER_HOME).join(path);
                let runner = AgentNodeRunnerConfig {
                    node_id: record.node_id.clone(),
                    control_plane_endpoint: managed.config.control_plane_endpoint.clone(),
                    credential_file: path_in_home(CREDENTIAL_PATH),
                    trusted_ca_file: managed.trusted_ca.as_ref().map(|_| path_in_home(CA_PATH)),
                    transport: None,
                };
                let mut files = vec![SeedFile::new(
                    "conf/talos.yaml",
                    talos_config::managed_worker_seed_config(runner)?,
                )];
                if let Some(ca) = &managed.trusted_ca {
                    files.push(SeedFile::new(CA_PATH, ca.clone()));
                }
                runtime
                    .ensure_home(
                        &spec,
                        managed.home_capacity(),
                        &files,
                        SeedMode::CreateMissing,
                    )
                    .await
                    .map_err(runtime_error)?;
                // No Worker can register at this checkpoint. If a crash separated the
                // database write from the seed, rotate again and replace only the credential.
                self.seed_managed_credential(record, &spec).await?;
                committed.home_ready = true;
                Step::Ensure
            }
            Step::Ensure => {
                runtime.ensure_runtime(&spec).await.map_err(runtime_error)?;
                if record.desired == Desired::Running {
                    Step::Start
                } else {
                    Step::Stable
                }
            }
            Step::Start => {
                let node = self
                    .store
                    .get(&record.node_id)
                    .await?
                    .ok_or_else(not_found)?;
                if node
                    .credential_expires_at
                    .is_none_or(|expires| expires <= now())
                {
                    return Err(TalosError::new(
                        ErrorCode::ResourceExpired,
                        "The Talo Worker credential has expired.",
                    ));
                }
                self.store
                    .set_status(SetNodeStatusParams {
                        node_id: record.node_id.clone(),
                        status: NodeStatus::Enabled,
                        now_at: now(),
                    })
                    .await?;
                runtime
                    .set_running(&spec, DesiredRuntimeState::Running)
                    .await
                    .map_err(runtime_error)?;
                Step::Wait
            }
            Step::Wait => {
                if self.managed_node_is_available(&record.node_id).await? {
                    committed.last_activity_at = now();
                    committed.error = None;
                    Step::Stable
                } else if record.error.is_some() {
                    return Ok(());
                } else if now() - record.operation_at >= START_TIMEOUT.as_secs() as i64 {
                    return Err(TalosError::new(
                        ErrorCode::Timeout,
                        "Timed out waiting for Talo Worker registration.",
                    ));
                } else {
                    return Ok(());
                }
            }
            Step::Stop => {
                stop_container(&**runtime, &spec).await?;
                Step::Stable
            }
            Step::RemoveForReset => {
                stop_container(&**runtime, &spec).await?;
                runtime.remove_runtime(&spec).await.map_err(runtime_error)?;
                committed.revision = record.revision.checked_add(1).ok_or_else(conflict)?;
                committed.image.clone_from(&managed.config.image);
                Step::Ensure
            }
            Step::RecoverCredential => {
                stop_container(&**runtime, &spec).await?;
                self.seed_managed_credential(record, &spec).await?;
                Step::Ensure
            }
            Step::RemoveForDelete => {
                stop_container(&**runtime, &spec).await?;
                runtime.remove_runtime(&spec).await.map_err(runtime_error)?;
                Step::RemoveHome
            }
            Step::RemoveHome => {
                runtime
                    .remove_home(&spec.identity)
                    .await
                    .map_err(runtime_error)?;
                Step::FinalizeDelete
            }
            Step::FinalizeDelete => {
                self.store
                    .finalize_managed_worker_deletion(record, now())
                    .await?;
                managed.changed.send_replace(());
                return Ok(());
            }
            Step::Stable => unreachable!(),
        };
        // Publish progress only after the corresponding host-side work succeeds.
        // Keep the caller's original checkpoint if persistence fails.
        committed.step = next;
        self.store
            .write_managed_worker(&committed, record.generation, Write::Checkpoint)
            .await?;
        // Wake clients may need the new checkpoint. The reconciler itself uses its
        // periodic tick for pending Wait so it cannot spin on an offline Worker.
        if next == Step::Stable {
            managed.changed.send_replace(());
        }
        Ok(())
    }

    async fn seed_managed_credential(
        &self,
        record: &ManagedWorkerRecord,
        spec: &WorkerSpec,
    ) -> TalosResult<()> {
        let token = new_token()?;
        self.store
            .rotate_credential(RotateCredentialParams {
                node_id: record.node_id.clone(),
                hash: hash_token(&token),
                expires_at: now() + CREDENTIAL_LIFETIME_SECONDS,
                now_at: now(),
            })
            .await?;
        let managed = self.managed_runtime()?;
        managed
            .runtime
            .ensure_home(
                spec,
                managed.home_capacity(),
                &[SeedFile::new(CREDENTIAL_PATH, token.into_bytes())],
                SeedMode::Replace,
            )
            .await
            .map_err(runtime_error)
    }
}
