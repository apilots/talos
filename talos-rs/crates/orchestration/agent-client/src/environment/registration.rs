use super::*;

impl AgentEnvironmentClient {
    /// Reserve idle Worker admission during a registration deletion; local host access remains independent.
    pub async fn set_registration_admission(
        &self,
        admission: agent_protocol::RegistrationAdmission,
    ) -> TalosResult<()> {
        use agent_protocol::RegistrationAdmission;
        use pb::set_registration_admission_request::ManagedDesktop;
        let managed_desktop = match admission {
            RegistrationAdmission::StopManaged => Some(ManagedDesktop::RejectController(true)),
            RegistrationAdmission::SleepManaged { idle_before } => {
                Some(ManagedDesktop::IdleBeforeUnixSeconds(idle_before))
            }
            RegistrationAdmission::Block | RegistrationAdmission::Unblock => None,
        };
        let request = pb::SetRegistrationAdmissionRequest {
            auth: Some(node_pb::encode_auth(self.auth.clone())),
            blocked: admission != RegistrationAdmission::Unblock,
            managed_desktop,
        };
        let mut request = Request::new(request);
        request.set_timeout(Duration::from_secs(10));
        let response = self
            .transport
            .set_registration_admission(request)
            .await
            .map_err(crate::decode_rpc_status)?
            .into_inner();
        match response.result.ok_or_else(wire::invalid)? {
            pb::empty_response::Result::Completed(_) => Ok(()),
            pb::empty_response::Result::Failure(error) => Err(error_pb::decode_failure(error)?),
        }
    }
}
