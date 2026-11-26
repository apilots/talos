//! Registration deletion reuses the Worker's original global admission gate.
use super::*;

impl AgentHarnessRpcAdapter {
    /// Renew the same lease in place, or replace a registration and cancel its
    /// old streams. Renewal must not change the execution's lease identity.
    pub fn install_registration(
        &self,
        registration: Registration,
        deadline: Instant,
    ) -> TalosResult<()> {
        self.install_node_registration(&self.node_id, registration, deadline)
    }

    /// Attach the control plane's verified UUID to this same execution runtime.
    /// The host alias and its private credential retain their independent lifetime.
    pub fn install_node_registration(
        &self,
        node_id: &str,
        registration: Registration,
        deadline: Instant,
    ) -> TalosResult<()> {
        if deadline <= Instant::now()
            || node_id.trim().is_empty()
            || registration.lease_token.is_empty()
            || registration.owner_user_id.is_empty()
        {
            return Err(invalid_request());
        }
        let lease = Lease {
            node_id: node_id.into(),
            token_hash: Sha256::digest(registration.lease_token.as_bytes()).into(),
            owner: AuthorizationOwner::User(registration.owner_user_id),
            deadline: watch::channel(deadline).0,
            revoked: CancellationToken::new(),
        };
        let mut current = self.lease.lock().expect("Agent lease lock poisoned");
        if let Some(previous) = current.as_ref() {
            if previous.node_id == lease.node_id
                && previous.token_hash == lease.token_hash
                && previous.owner == lease.owner
            {
                previous.deadline.send_replace(deadline);
                return Ok(());
            }
        }
        if let Some(previous) = current.replace(lease) {
            previous.revoked.cancel();
            if let Some(revoked) = &self.registration_revoked {
                revoked();
            }
        }
        Ok(())
    }

    /// Attach the Worker desktop check to the existing registration fence.
    #[must_use]
    pub fn with_managed_desktop_admission(mut self, admission: ManagedDesktopAdmission) -> Self {
        self.managed_desktop_admission = Some(admission);
        self
    }

    /// Hosts attach node-local authorization cancellation to the existing lease lifecycle.
    #[must_use]
    pub fn with_registration_revocation_hook(mut self, hook: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.registration_revoked = Some(hook);
        self
    }

    pub fn clear_registration(&self) {
        if let Some(previous) = self.lease.lock().expect("Agent lease lock poisoned").take() {
            previous.revoked.cancel();
            if let Some(revoked) = &self.registration_revoked {
                revoked();
            }
        }
    }

    /// A protected host credential permits the host's already-authorized users.
    /// It is never advertised or sent through the cloud registration connection.
    pub fn install_local_auth(&self, auth: &NodeAuth) -> TalosResult<()> {
        if auth.node_id != self.node_id || auth.token.is_empty() {
            return Err(invalid_request());
        }
        let lease = Lease {
            node_id: auth.node_id.clone(),
            token_hash: Sha256::digest(auth.token.as_bytes()).into(),
            owner: AuthorizationOwner::Host,
            deadline: watch::channel(Instant::now()).0,
            revoked: CancellationToken::new(),
        };
        if let Some(previous) = self
            .local_auth
            .lock()
            .expect("Local authorization lock poisoned")
            .replace(lease)
        {
            previous.revoked.cancel();
        }
        Ok(())
    }

    pub fn clear_local_auth(&self) {
        if let Some(previous) = self
            .local_auth
            .lock()
            .expect("Local authorization lock poisoned")
            .take()
        {
            previous.revoked.cancel();
        }
    }

    pub(super) fn authorize_admission(&self, lease: &Lease) -> TalosResult<()> {
        if lease.revoked.is_cancelled()
            || (lease.owner != AuthorizationOwner::Host
                && self
                    .blocked_registration
                    .lock()
                    .expect("Registration admission lock")
                    .as_ref()
                    == Some(&lease.token_hash))
        {
            return Err(TalosError::new(
                ErrorCode::ResourceExpired,
                "Computer registration is being removed.",
            ));
        }
        Ok(())
    }

    pub(super) async fn registration_admission(
        &self,
        request: pb::SetRegistrationAdmissionRequest,
    ) -> TalosResult<()> {
        // Carrier authorization remains valid for acknowledgements and rollback,
        // while normal remote product calls use authorize_admission as well.
        let lease = self.authorize_transport(&node_pb::decode_auth(request.auth)?)?;
        if lease.owner == AuthorizationOwner::Host {
            return Err(invalid_request());
        }
        let _reservation = if request.blocked {
            Some(self.session_gate.reserve_configuration().map_err(|error| {
                if error.is_code(ErrorCode::ActiveTurnRunning) {
                    TalosError::localized(talos_error::ErrorLocalization::ComputerDeletionActive)
                        .with_source(error)
                } else {
                    error
                }
            })?)
        } else {
            None
        };
        use agent_protocol::RegistrationAdmission;
        use pb::set_registration_admission_request::ManagedDesktop;
        let desktop = match (request.blocked, request.managed_desktop) {
            (true, Some(ManagedDesktop::RejectController(true))) => {
                Some(RegistrationAdmission::StopManaged)
            }
            (true, Some(ManagedDesktop::IdleBeforeUnixSeconds(idle_before))) => {
                Some(RegistrationAdmission::SleepManaged { idle_before })
            }
            (false, None) => Some(RegistrationAdmission::Unblock),
            (true, None) => None,
            _ => return Err(invalid_request()),
        };
        match (desktop, &self.managed_desktop_admission) {
            (Some(admission), Some(probe)) => probe(admission, lease.revoked.clone()).await?,
            (
                Some(
                    RegistrationAdmission::StopManaged | RegistrationAdmission::SleepManaged { .. },
                ),
                None,
            ) => {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Talo desktop admission is unavailable.",
                ));
            }
            _ => {}
        }
        *self
            .blocked_registration
            .lock()
            .expect("Registration admission lock") = request.blocked.then_some(lease.token_hash);
        Ok(())
    }
}

#[cfg(test)]
#[path = "rpc_registration_tests.rs"]
mod tests;
