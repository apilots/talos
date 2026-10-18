//! Registration deletion reuses the Worker's original global admission gate.
use super::*;

impl AgentHarnessRpcAdapter {
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

    pub(super) fn registration_admission(
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
