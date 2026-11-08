//! Talo lifecycle intent belongs to the Computer Service. Runtime handles only host resources.

#[path = "managed_worker/reconcile.rs"]
mod reconcile;

use super::*;
use crate::worker_runtime_settings::KUBERNETES_HOME_BYTES;
use std::time::Duration;
use talos_config::{
    domains::managed_worker::WorkerRuntimeKind, ServerRuntimePolicy, WorkerRuntimeConfig,
};
use talos_store::node::{
    ManagedWorkerAction as Action, ManagedWorkerDesired as Desired, ManagedWorkerRecord,
    ManagedWorkerStep as Step, ManagedWorkerWrite as Write,
};
use talos_types::{
    DeleteManagedWorkerRequest, ManagedWorkerActionRequest, ManagedWorkerDiskQuota,
    ManagedWorkerInfo, ManagedWorkerOperation, ManagedWorkerPhase as Phase, ManagedWorkerResponse,
};
use talos_worker_runtime::{
    ContainerState, DesiredRuntimeState, HomeCapacity, ManagedWorkerRuntime, RuntimeError,
    RuntimeIdentity, WorkerSpec,
};
use tokio::sync::watch;

const CPU_CORES: u16 = 4;
const MEMORY_BYTES: u64 = 8 * 1024 * 1024 * 1024;

const WAKE_COOLDOWN_SECONDS: i64 = 60;
const REQUEST_WAIT: Duration = Duration::from_secs(45);
const START_TIMEOUT: Duration = Duration::from_secs(120);

/// Existing dispatch may proceed or defer its current candidate during shared Wake.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagedDispatch {
    Proceed,
    Waking,
    Deferred,
}

pub(super) struct ManagedWorkers {
    runtime: Arc<dyn ManagedWorkerRuntime>,
    config: WorkerRuntimeConfig,
    policy: ServerRuntimePolicy,
    trusted_ca: Option<Vec<u8>>,
    changed: watch::Sender<()>,
}

impl ManagedWorkers {
    fn home_capacity(&self) -> HomeCapacity {
        match self.config.kind {
            WorkerRuntimeKind::DockerEngine => HomeCapacity::Unrestricted,
            WorkerRuntimeKind::Kubernetes => HomeCapacity::LimitedBytes(KUBERNETES_HOME_BYTES),
        }
    }

    fn disk_quota(&self) -> ManagedWorkerDiskQuota {
        if self.config.kind == WorkerRuntimeKind::Kubernetes
            && self.config.kubernetes.home_quota_enforced
        {
            ManagedWorkerDiskQuota::Enforced
        } else {
            ManagedWorkerDiskQuota::Unsupported
        }
    }
}

impl AgentNodeService {
    /// Serialize saving the next Server Runtime with Talo creation.
    /// The callback receives whether any managed resource record exists and
    /// returns whether the persisted settings are pending restart.
    pub async fn with_managed_runtime_update<T, F>(
        &self,
        operation: impl FnOnce(bool) -> F,
    ) -> TalosResult<T>
    where
        F: std::future::Future<Output = TalosResult<(T, bool)>> + Send,
    {
        let mut pending = self.managed_submission.lock().await;
        let (result, restart_required) = operation(self.store.has_managed_workers().await?).await?;
        *pending = restart_required;
        Ok(result)
    }

    /// Configure the runtime once, on the authenticated management Server only.
    pub fn install_managed_worker_runtime(
        &self,
        runtime: Arc<dyn ManagedWorkerRuntime>,
        config: WorkerRuntimeConfig,
        policy: ServerRuntimePolicy,
        trusted_ca: Option<Vec<u8>>,
    ) -> TalosResult<()> {
        self.managed
            .set(ManagedWorkers {
                runtime,
                config,
                policy,
                trusted_ca,
                changed: watch::channel(()).0,
            })
            .map_err(|_| conflict())
    }

    pub fn managed_worker_enabled(&self) -> bool {
        self.managed.get().is_some()
    }

    pub(crate) fn admission_gates(&self) -> Arc<crate::turn::NodeAdmissionGates> {
        self.admission.clone()
    }

    /// Reads the single lifecycle record scoped to the signed-in account.
    pub async fn read_managed_worker(&self, actor: &Actor) -> TalosResult<ManagedWorkerResponse> {
        let instance = if actor.auth_disabled {
            None
        } else {
            ensure_management_actor(actor)?;
            match self.store.managed_worker(&actor.user_id).await? {
                Some(record) => Some(self.managed_worker_info(&record).await?),
                None => None,
            }
        };
        Ok(ManagedWorkerResponse {
            enabled: self.managed_worker_enabled() && !actor.auth_disabled,
            instance,
        })
    }

    /// Reserves the original node and Home identity atomically before any host action.
    pub async fn create_managed_worker(&self, actor: &Actor) -> TalosResult<ManagedWorkerInfo> {
        ensure_management_actor(actor)?;
        let managed = self.managed_runtime()?;
        let pending = self.managed_submission.lock().await;
        if let Some(record) = self.store.managed_worker(&actor.user_id).await? {
            return self.managed_worker_info(&record).await;
        }
        if *pending {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "The Worker Runtime has a saved change awaiting Server restart.",
            ));
        }
        let now_at = now();
        let record = ManagedWorkerRecord {
            instance_id: uuid::Uuid::new_v4().to_string(),
            owner_user_id: actor.user_id.clone(),
            node_id: uuid::Uuid::new_v4().to_string(),
            conversation_session_id: format!("sess_{}", uuid::Uuid::new_v4().simple()),
            short_id: uuid::Uuid::new_v4().simple().to_string()[..8].to_owned(),
            image: managed.config.image.clone(),
            revision: 1,
            generation: 1,
            desired: Desired::Running,
            operation: Some(Action::Create),
            step: Step::Provision,
            home_ready: false,
            last_activity_at: now_at,
            operation_at: now_at,
            error: None,
        };
        self.store.reserve_managed_worker(&record).await?;
        managed.changed.send_replace(());
        Ok(worker_info(&record, false, managed.disk_quota()))
    }

    pub async fn start_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        self.submit_managed_worker_action(actor, request, Action::Start)
            .await
    }

    pub async fn stop_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        self.submit_managed_worker_action(actor, request, Action::Stop)
            .await
    }

    pub async fn reset_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        self.submit_managed_worker_action(actor, request, Action::Reset)
            .await
    }

    pub async fn delete_managed_worker(
        &self,
        actor: &Actor,
        request: DeleteManagedWorkerRequest,
    ) -> TalosResult<ManagedWorkerInfo> {
        if !request.confirm_data_loss {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Confirm permanent deletion of the Talo Home.",
            ));
        }
        self.submit_managed_worker_action(
            actor,
            ManagedWorkerActionRequest {
                instance_id: request.instance_id,
                expected_generation: request.expected_generation,
            },
            Action::Delete,
        )
        .await
    }

    async fn submit_managed_worker_action(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
        action: Action,
    ) -> TalosResult<ManagedWorkerInfo> {
        ensure_management_actor(actor)?;
        let managed = self.managed_runtime()?;
        let _submit = self.managed_submission.lock().await;
        let Some(mut record) = self
            .store
            .managed_worker(&actor.user_id)
            .await?
            .filter(|record| record.instance_id == request.instance_id)
        else {
            let receipt = if action == Action::Delete {
                self.store
                    .managed_worker_deletion(&actor.user_id, &request.instance_id)
                    .await?
            } else {
                None
            };
            if let Some(receipt) = receipt.filter(|receipt| {
                receipt.generation == request.expected_generation
                    || receipt.generation.checked_sub(1) == Some(request.expected_generation)
            }) {
                return Ok(ManagedWorkerInfo {
                    instance_id: receipt.instance_id,
                    node_id: receipt.node_id,
                    conversation_session_id: None,
                    generation: receipt.generation,
                    phase: Phase::Deleting,
                    operation: Some(ManagedWorkerOperation::Delete),
                    cpu_cores: CPU_CORES,
                    memory_bytes: MEMORY_BYTES,
                    disk_quota: managed.disk_quota(),
                    error: None,
                });
            }
            return Err(not_found());
        };
        let pending = record.step != Step::Stable && record.error.is_none();
        let same_action = record.operation == Some(action)
            || (action == Action::Start
                && matches!(record.operation, Some(Action::Create | Action::Wake)));
        if same_action
            && record.error.is_none()
            && (pending || record.generation.checked_sub(1) == Some(request.expected_generation))
            && (record.generation == request.expected_generation
                || record.generation.checked_sub(1) == Some(request.expected_generation))
        {
            return self.managed_worker_info(&record).await;
        }
        if record.generation != request.expected_generation
            || (record.desired == Desired::Deleting && action != Action::Delete)
            || (pending && !(record.operation == Some(Action::Wake) && record.step == Step::Wait))
        {
            return Err(conflict());
        }
        if action == Action::Start
            && record.step == Step::Stable
            && record.desired == Desired::Running
            && self.managed_node_is_available(&record.node_id).await?
        {
            return Ok(worker_info(&record, true, managed.disk_quota()));
        }
        let expected_generation = record.generation;
        let retry = record.error.is_some();
        let previous_action = record.operation;
        update_intent(&mut record, action)?;
        match action {
            Action::Start => {
                if retry && record.step == Step::RemoveForReset {
                    record.operation = Some(Action::Reset);
                }
                record.desired = Desired::Running;
                if !record.home_ready {
                    record.step = Step::Provision;
                } else if !matches!(record.step, Step::RemoveForReset | Step::RecoverCredential) {
                    let node = self
                        .store
                        .get(&record.node_id)
                        .await?
                        .ok_or_else(not_found)?;
                    record.step = if node
                        .credential_expires_at
                        .is_some_and(|expires| expires > now())
                    {
                        Step::Ensure
                    } else {
                        Step::RecoverCredential
                    };
                }
                if record.step == Step::RecoverCredential {
                    self.suspend_managed_worker(&record, expected_generation)
                        .await?;
                } else {
                    self.store
                        .write_managed_worker(&record, expected_generation, Write::Checkpoint)
                        .await?;
                }
            }
            Action::Stop | Action::Reset | Action::Delete => {
                match action {
                    Action::Stop => {
                        record.desired = Desired::ManualStop;
                        record.step = Step::Stop;
                    }
                    Action::Reset => {
                        if !(retry && previous_action == Some(Action::Reset)) {
                            record.step = if !record.home_ready {
                                Step::Provision
                            } else {
                                Step::RemoveForReset
                            };
                        }
                    }
                    Action::Delete => {
                        record.desired = Desired::Deleting;
                        if !(retry && previous_action == Some(Action::Delete)) {
                            record.step = Step::RemoveForDelete;
                        }
                    }
                    _ => unreachable!(),
                }
                self.suspend_managed_worker(&record, expected_generation)
                    .await?;
            }
            Action::Create | Action::Wake | Action::Sleep => unreachable!(),
        }
        managed.changed.send_replace(());
        Ok(worker_info(&record, false, managed.disk_quota()))
    }

    async fn suspend_managed_worker(
        &self,
        record: &ManagedWorkerRecord,
        expected_generation: u32,
    ) -> TalosResult<()> {
        let _admission = self
            .admission
            .for_node(&record.node_id)
            .try_write_owned()
            .map_err(|_| busy())?;
        self.suspend_managed_worker_with_admission(record, expected_generation)
            .await
    }

    async fn suspend_managed_worker_with_admission(
        &self,
        record: &ManagedWorkerRecord,
        expected_generation: u32,
    ) -> TalosResult<()> {
        let _lifecycle = self.lock_lifecycle().await;
        let prepare = self
            .current
            .lock()
            .await
            .get(&record.node_id)
            .and_then(|registration| registration.prepare_deletion.clone());
        if let Some(prepare) = &prepare {
            let admission = if record.operation == Some(Action::Sleep) {
                agent_protocol::RegistrationAdmission::SleepManaged {
                    idle_before: now()
                        - i64::from(self.managed_runtime()?.policy.idle_timeout_seconds),
                }
            } else {
                agent_protocol::RegistrationAdmission::StopManaged
            };
            match prepare(admission).await {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.error_code(),
                        ErrorCode::Unauthenticated
                            | ErrorCode::ResourceExpired
                            | ErrorCode::ServiceUnavailable
                            | ErrorCode::Timeout
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        if let Err(error) = self
            .store
            .write_managed_worker(record, expected_generation, Write::Suspend)
            .await
        {
            if let Some(prepare) = prepare {
                if let Err(cleanup) = prepare(agent_protocol::RegistrationAdmission::Unblock).await
                {
                    talos_logging::report_error(&cleanup);
                }
            }
            return Err(error);
        }
        let previous = self.current.lock().await.remove(&record.node_id);
        revoke_registration(previous).await;
        Ok(())
    }

    /// Indicates which Computer is governed by this Server's lifecycle policy.
    pub async fn is_managed_worker_node(&self, node_id: &str) -> TalosResult<bool> {
        Ok(self.store.managed_worker_by_node(node_id).await?.is_some())
    }

    pub(super) async fn reject_managed_node_mutation(&self, node_id: &str) -> TalosResult<()> {
        if self.is_managed_worker_node(node_id).await? {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Manage Talo through its lifecycle controls.",
            ));
        }
        Ok(())
    }

    pub(super) async fn managed_node_is_available(&self, node_id: &str) -> TalosResult<bool> {
        match self.store.get(node_id).await? {
            Some(node) => Ok(self.node_info(node).await.available),
            None => Ok(false),
        }
    }

    pub(crate) async fn managed_conversation_node(
        &self,
        actor: &Actor,
        request: &talos_types::StartTurnRequest,
    ) -> TalosResult<Option<String>> {
        let Some(record) = self
            .store
            .managed_worker_by_conversation(&request.session_id)
            .await?
        else {
            return Ok(None);
        };
        ensure_management_actor(actor)?;
        if record.owner_user_id != actor.user_id {
            return Err(not_found());
        }
        if request.project_id.is_some()
            || request
                .node_id
                .as_deref()
                .is_some_and(|node| node != record.node_id)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "The Talo conversation belongs to its Talo and cannot use a Project.",
            ));
        }
        Ok(Some(record.node_id))
    }

    pub(crate) async fn managed_request_wait(&self, node_id: &str) -> TalosResult<Duration> {
        if self.managed_worker_enabled()
            && node_id != "embedded"
            && self.is_managed_worker_node(node_id).await?
        {
            return Ok(REQUEST_WAIT);
        }
        Ok(Duration::ZERO)
    }

    /// Active requests can wait for shared Wake without owning the Docker operation.
    pub(crate) async fn acquire_managed_worker(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<Option<tokio::sync::OwnedRwLockReadGuard<()>>> {
        if !self.managed_worker_enabled() || node_id == "embedded" {
            return Ok(None);
        }
        let Some(record) = self.store.managed_worker_by_node(node_id).await? else {
            return Ok(None);
        };
        ensure_management_actor(actor)?;
        ensure_owner(&record, &actor.user_id)?;
        let generation = self
            .wait_managed_worker(&actor.user_id, node_id, REQUEST_WAIT)
            .await?;
        let admission = self.admission.for_node(node_id).read_owned().await;
        let current = self
            .store
            .managed_worker_by_node(node_id)
            .await?
            .ok_or_else(not_found)?;
        ensure_owner(&current, &actor.user_id)?;
        if current.generation != generation
            || current.desired != Desired::Running
            || !self.managed_node_is_available(node_id).await?
        {
            return Err(unavailable());
        }
        self.store
            .record_managed_worker_activity(node_id, now())
            .await?;
        Ok(Some(admission))
    }

    /// Existing Automation and queue dispatch check this only after qualifying a candidate.
    /// Unmanaged, manually stopped and ordinarily disconnected targets keep original policy.
    pub(crate) async fn wake_for_managed_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<ManagedDispatch> {
        let Some(managed) = self.managed.get().filter(|_| node_id != "embedded") else {
            return Ok(ManagedDispatch::Proceed);
        };
        let Some(record) = self.store.managed_worker_by_node(node_id).await? else {
            return Ok(ManagedDispatch::Proceed);
        };
        ensure_owner(&record, owner)?;
        if record.desired != Desired::IdleSleep {
            return Ok(
                if record.operation == Some(Action::Wake) && record.step != Step::Stable {
                    if record.error.is_some() {
                        ManagedDispatch::Deferred
                    } else {
                        ManagedDispatch::Waking
                    }
                } else {
                    ManagedDispatch::Proceed
                },
            );
        }
        let _submit = self.managed_submission.lock().await;
        let Some(mut record) = self.store.managed_worker_by_node(node_id).await? else {
            return Err(not_found());
        };
        ensure_owner(&record, owner)?;
        if record.desired != Desired::IdleSleep {
            return Ok(
                if record.operation == Some(Action::Wake) && record.step != Step::Stable {
                    ManagedDispatch::Waking
                } else {
                    ManagedDispatch::Proceed
                },
            );
        }
        if record.step != Step::Stable {
            return Ok(if record.error.is_some() {
                ManagedDispatch::Deferred
            } else {
                ManagedDispatch::Waking
            });
        }
        if record.error.is_some() && now() - record.operation_at < WAKE_COOLDOWN_SECONDS {
            return Ok(ManagedDispatch::Deferred);
        }
        let expected_generation = record.generation;
        update_intent(&mut record, Action::Wake)?;
        record.desired = Desired::Running;
        record.step = Step::Ensure;
        self.store
            .write_managed_worker(&record, expected_generation, Write::Checkpoint)
            .await?;
        managed.changed.send_replace(());
        Ok(ManagedDispatch::Waking)
    }

    /// Accepted queue items and qualified automation share the lifecycle operation's wait budget.
    pub(crate) async fn wait_managed_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<()> {
        self.wait_managed_worker(owner, node_id, START_TIMEOUT)
            .await
            .map(|_| ())
    }

    /// A successful return names the checked operation generation.
    async fn wait_managed_worker(
        &self,
        owner: &str,
        node_id: &str,
        timeout: Duration,
    ) -> TalosResult<u32> {
        let mut changed = self.managed_runtime()?.changed.subscribe();
        let result = tokio::time::timeout(timeout, async {
            let mut wake_generation = None;
            loop {
                let dispatch = self.wake_for_managed_dispatch(owner, node_id).await?;
                let record = self
                    .store
                    .managed_worker_by_node(node_id)
                    .await?
                    .ok_or_else(not_found)?;
                ensure_owner(&record, owner)?;
                if wake_generation.is_some_and(|generation| generation != record.generation) {
                    return Err(unavailable());
                }
                if record.operation == Some(Action::Wake) {
                    wake_generation = Some(record.generation);
                }
                if record.desired == Desired::Running
                    && record.step == Step::Stable
                    && self.managed_node_is_available(node_id).await?
                {
                    return Ok(record.generation);
                }
                if dispatch == ManagedDispatch::Deferred {
                    return Err(record
                        .error
                        .map(|error| TalosError::from(error.0))
                        .unwrap_or_else(unavailable));
                }
                if dispatch == ManagedDispatch::Proceed
                    && (record.desired != Desired::Running
                        || record.error.is_some()
                        || record.step == Step::Stable)
                {
                    return Err(unavailable());
                }
                tokio::select! {
                    _ = changed.changed() => {},
                    () = tokio::time::sleep(Duration::from_millis(500)) => {},
                }
            }
        })
        .await;
        result.map_err(|_| {
            TalosError::new(ErrorCode::Timeout, "Timed out waiting for Talo to start.")
        })?
    }

    pub(crate) async fn record_managed_activity(&self, node_id: &str) -> TalosResult<()> {
        if self.managed_worker_enabled() && node_id != "embedded" {
            self.store
                .record_managed_worker_activity(node_id, now())
                .await?;
        }
        Ok(())
    }

    async fn managed_worker_info(
        &self,
        record: &ManagedWorkerRecord,
    ) -> TalosResult<ManagedWorkerInfo> {
        let available = self.managed_node_is_available(&record.node_id).await?;
        let quota = self.managed.get().map_or(
            ManagedWorkerDiskQuota::Unsupported,
            ManagedWorkers::disk_quota,
        );
        Ok(worker_info(record, available, quota))
    }

    fn managed_runtime(&self) -> TalosResult<&ManagedWorkers> {
        self.managed.get().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Talo is not configured on this Server.",
            )
        })
    }
}

pub(super) fn worker_phase(record: &ManagedWorkerRecord, available: bool) -> Phase {
    match (
        record.desired,
        record.step,
        record.operation,
        record.error.is_some(),
    ) {
        (Desired::Running, Step::Stable, _, _) if available => Phase::Ready,
        (Desired::IdleSleep, Step::Stable, _, _) => Phase::Sleeping,
        (_, _, _, true) => Phase::Failed,
        (Desired::Deleting, _, _, _) => Phase::Deleting,
        (_, Step::Stable, _, _) if record.desired == Desired::ManualStop => Phase::Stopped,
        (_, Step::Stable, _, _) => Phase::Offline,
        (_, _, Some(Action::Reset), _) if !matches!(record.step, Step::Wait) => Phase::Resetting,
        (_, Step::Provision, _, _) => Phase::Provisioning,
        (_, Step::Ensure | Step::Start | Step::RecoverCredential, _, _) => Phase::Starting,
        (_, Step::Wait, _, _) => Phase::WaitingForRegistration,
        (_, Step::Stop, _, _) => Phase::Stopping,
        (_, Step::RemoveForReset, _, _) => Phase::Resetting,
        (_, Step::RemoveForDelete | Step::RemoveHome | Step::FinalizeDelete, _, _) => {
            Phase::Deleting
        }
    }
}

fn worker_info(
    record: &ManagedWorkerRecord,
    available: bool,
    disk_quota: ManagedWorkerDiskQuota,
) -> ManagedWorkerInfo {
    let phase = worker_phase(record, available);
    let operation = record.operation.map(|action| match action {
        Action::Create => ManagedWorkerOperation::Create,
        Action::Start | Action::Wake => ManagedWorkerOperation::Start,
        Action::Stop | Action::Sleep => ManagedWorkerOperation::Stop,
        Action::Reset => ManagedWorkerOperation::Reset,
        Action::Delete => ManagedWorkerOperation::Delete,
    });
    ManagedWorkerInfo {
        instance_id: record.instance_id.clone(),
        node_id: record.node_id.clone(),
        conversation_session_id: Some(record.conversation_session_id.clone()),
        generation: record.generation,
        phase,
        operation: if record.step == Step::Stable && record.error.is_none() {
            None
        } else {
            operation
        },
        cpu_cores: CPU_CORES,
        memory_bytes: MEMORY_BYTES,
        disk_quota,
        error: if phase == Phase::Ready {
            None
        } else {
            record.error.as_ref().map(|error| (**error).clone())
        },
    }
}

fn update_intent(record: &mut ManagedWorkerRecord, action: Action) -> TalosResult<()> {
    record.generation = record.generation.checked_add(1).ok_or_else(conflict)?;
    record.operation = Some(action);
    record.operation_at = now();
    record.error = None;
    Ok(())
}

fn runtime_spec(record: &ManagedWorkerRecord) -> TalosResult<WorkerSpec> {
    Ok(WorkerSpec {
        identity: RuntimeIdentity {
            instance_id: uuid::Uuid::parse_str(&record.instance_id).map_err(|error| {
                TalosError::internal_with_source("Invalid persisted Talo identity.", error)
            })?,
            short_id: record.short_id.clone(),
        },
        revision: u64::try_from(record.revision).map_err(|error| {
            TalosError::internal_with_source("Invalid persisted Talo revision.", error)
        })?,
        image: record.image.clone(),
        cpu_cores: u32::from(CPU_CORES),
        memory_bytes: MEMORY_BYTES,
    })
}

async fn stop_container(runtime: &dyn ManagedWorkerRuntime, spec: &WorkerSpec) -> TalosResult<()> {
    runtime
        .set_running(spec, DesiredRuntimeState::Stopped)
        .await
        .map_err(runtime_error)
}

pub(super) fn runtime_error(error: RuntimeError) -> TalosError {
    let code = match &error {
        RuntimeError::InvalidSpec(_) | RuntimeError::UnsupportedCapacity => {
            ErrorCode::InvalidRequest
        }
        RuntimeError::Conflict { .. } => ErrorCode::Conflict,
        RuntimeError::Missing { .. }
        | RuntimeError::UnexpectedState { .. }
        | RuntimeError::Docker { .. }
        | RuntimeError::Kubernetes { .. }
        | RuntimeError::SeedIo(_) => ErrorCode::ServiceUnavailable,
    };
    TalosError::from_source(code, "The Talo Worker runtime operation failed.", error)
}

fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The Talo lifecycle changed. Refresh and try again.",
    )
}

fn ensure_owner(record: &ManagedWorkerRecord, owner: &str) -> TalosResult<()> {
    if record.owner_user_id != owner {
        return Err(not_found());
    }
    Ok(())
}

fn unavailable() -> TalosError {
    TalosError::new(ErrorCode::ServiceUnavailable, "Talo is unavailable.")
}

fn busy() -> TalosError {
    TalosError::new(ErrorCode::Conflict, "Talo is handling an active request.")
}
