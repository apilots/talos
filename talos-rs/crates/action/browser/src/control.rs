//! Revision-bound browser control transitions. State remains owned by BrowserRuntime.
use super::*;
impl BrowserRuntime {
    pub async fn operate(
        self: &Arc<Self>,
        user_id: String,
        operation: BrowserOperation,
    ) -> TalosResult<BrowserSnapshot> {
        if matches!(operation, BrowserOperation::Read) {
            let backend = self.backend.read().map_err(|_| poisoned())?.clone();
            if let Some(backend) = backend.filter(|_| require_private_ipc_support().is_ok()) {
                backend.refresh(user_id).await?;
            }
            return self.snapshot();
        }
        if !(self.enabled)()? {
            return Err(unavailable(
                "Browser is disabled. Enable it in MCP settings.",
            ));
        }
        match operation {
            BrowserOperation::Read => {}
            BrowserOperation::SelectTarget {
                target_id: target,
                expected_revision,
            } => {
                let mut s = self.state.lock().map_err(|_| poisoned())?;
                revision(&s, expected_revision)?;
                if !s.invocations.is_empty()
                    || matches!(
                        s.owner,
                        BrowserControlOwner::Agent { .. }
                            | BrowserControlOwner::Human { .. }
                            | BrowserControlOwner::HandingOver { .. }
                    )
                {
                    return Err(conflict("Stop browser actions before changing its target."));
                }
                let target = match target.as_str() {
                    "managed" => BrowserTargetKind::Managed,
                    "connected" => BrowserTargetKind::Connected,
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::InvalidRequest,
                            "Unknown browser target.",
                        ))
                    }
                };
                if s.host == BrowserHostKind::Server
                    || (s.host == BrowserHostKind::Docker && target != BrowserTargetKind::Managed)
                    || (s.host == BrowserHostKind::Native && target != BrowserTargetKind::Connected)
                {
                    return Err(TalosError::new(
                        ErrorCode::InvalidRequest,
                        "This Computer does not support that browser target.",
                    ));
                }
                let mut next = s.preferences.clone();
                next.selected = target;
                self.persist(&next)?;
                s.preferences = next;
                s.revision += 1;
            }
            BrowserOperation::Pause { expected_revision } => {
                self.pause(Some(expected_revision), BrowserSuspensionReason::UserPaused)
                    .await?;
            }
            BrowserOperation::Resume { expected_revision } => {
                self.operate_resume(expected_revision).await?;
                self.state
                    .lock()
                    .map_err(|_| poisoned())?
                    .revoked_users
                    .remove(&user_id);
            }

            BrowserOperation::Connect { expected_revision }
            | BrowserOperation::Disconnect { expected_revision } => {
                let _input = self.input.clone().lock_owned().await;
                {
                    let s = self.state.lock().map_err(|_| poisoned())?;
                    revision(&s, expected_revision)?;
                    if s.preferences.selected != BrowserTargetKind::Connected
                        || !s.invocations.is_empty()
                    {
                        return Err(conflict("Select an idle user Chrome target first."));
                    }
                }
                if matches!(operation, BrowserOperation::Connect { .. }) {
                    require_private_ipc_support()?;
                    self.backend()?.connect(user_id.clone()).await?;
                } else {
                    self.backend()?.disconnect(user_id.clone()).await?;
                }
                let mut s = self.state.lock().map_err(|_| poisoned())?;
                revision(&s, expected_revision)?;
                s.revision += 1;
            }
            BrowserOperation::OpenLocal => {
                require_private_ipc_support()?;
                let unavailable = {
                    let s = self.state.lock().map_err(|_| poisoned())?;
                    s.host != BrowserHostKind::Desktop
                        || s.preferences.selected != BrowserTargetKind::Managed
                };
                if unavailable {
                    return Err(TalosError::localized(
                        talos_error::ErrorLocalization::BrowserCapabilityUnavailable {
                            operation: talos_error::BrowserCapabilityOperation::OpenLocal,
                            reason: talos_error::BrowserCapabilityReason::ClientMismatch,
                        },
                    ));
                }
                self.backend()?.open_local(user_id.clone()).await?;
            }
            BrowserOperation::TakeControl { expected_revision } => {
                self.require_desktop()?;
                // Warm only this controlled Docker display before starting the handoff deadline.
                drop(self.backend()?.desktop().await?);
                let paused_revision = self
                    .pause(Some(expected_revision), BrowserSuspensionReason::UserPaused)
                    .await?;
                let controller_id = Uuid::new_v4().to_string();
                let epoch = {
                    let mut s = self.state.lock().map_err(|_| poisoned())?;
                    revision(&s, paused_revision)?;
                    s.owner = BrowserControlOwner::HandingOver {
                        controller_id: controller_id.clone(),
                    };
                    s.revision += 1;
                    s.epoch
                };
                let runtime = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let pending = runtime.state.lock().ok().and_then(|s| {
                        (s.epoch == epoch && matches!(&s.owner, BrowserControlOwner::HandingOver {controller_id:id} if id == &controller_id))
                            .then_some(s.revision)
                    });
                    if let Some(revision) = pending {
                        if let Err(error) = runtime
                            .pause(Some(revision), BrowserSuspensionReason::TakeoverFailed)
                            .await
                        {
                            talos_logging::report_error(&error);
                        }
                    }
                });
            }
            BrowserOperation::ReleaseControl {
                controller_id,
                expected_revision,
            } => {
                self.require_desktop()?;
                {
                    let s = self.state.lock().map_err(|_| poisoned())?;
                    revision(&s, expected_revision)?;
                    if !matches!(&s.owner, BrowserControlOwner::Human {controller_id:id} | BrowserControlOwner::HandingOver {controller_id:id} if id == &controller_id)
                    {
                        return Err(denied());
                    }
                }
                let paused_revision = self
                    .pause(Some(expected_revision), BrowserSuspensionReason::UserPaused)
                    .await?;
                // Explicit release returns ownership to Agent admission after old input settles.
                self.operate_resume(paused_revision).await?;
            }
        }
        self.snapshot()
    }

    async fn operate_resume(&self, expected_revision: u64) -> TalosResult<()> {
        self.check_desktop_cleanup()?;
        let _input = self.input.clone().lock_owned().await;
        {
            let s = self.state.lock().map_err(|_| poisoned())?;
            revision(&s, expected_revision)?;
            if matches!(
                s.owner,
                BrowserControlOwner::Human { .. } | BrowserControlOwner::HandingOver { .. }
            ) {
                return Err(conflict(
                    "Release remote control before resuming automation.",
                ));
            }
        }
        if let Ok(backend) = self.backend() {
            backend.release_inputs().await?;
        }
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        revision(&s, expected_revision)?;
        let mut next = s.preferences.clone();
        next.manual_hold = false;
        self.persist(&next)?;
        s.preferences = next;
        revoke(&mut s);
        s.owner = BrowserControlOwner::Idle;
        s.agent_user = None;
        self.mutations.reopen();
        Ok(())
    }

    pub(super) async fn pause(
        &self,
        expected_revision: Option<u64>,
        reason: BrowserSuspensionReason,
    ) -> TalosResult<u64> {
        let (persisted, paused_revision) = {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            if let Some(expected) = expected_revision {
                revision(&s, expected)?;
            }
            s.preferences.manual_hold = true;
            revoke(&mut s);
            s.owner = BrowserControlOwner::Suspended { reason };
            self.mutations.close();
            (self.persist(&s.preferences), s.revision)
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            self.mutations.wait().await;
            self.check_desktop_cleanup()?;
            let _input = self.input.clone().lock_owned().await;
            if let Ok(backend) = self.backend() {
                backend.release_inputs().await?;
            }
            Ok::<_, TalosError>(())
        })
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Browser input could not be fenced in time.",
                error,
            )
        })??;
        persisted?;
        Ok(paused_revision)
    }
}
