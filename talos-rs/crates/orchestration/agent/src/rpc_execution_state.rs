use super::*;
use agent_protocol::{execution_state_pb, SessionStateVisibility};
impl AgentHarnessRpcAdapter {
    pub(super) fn coordinator_id<'a>(
        &self,
        caller: Option<&'a pb::CoordinatorCallContext>,
        session: &agent_protocol::AgentSessionContext,
    ) -> TalosResult<&'a str> {
        let caller = caller.ok_or_else(invalid_request)?;
        let scope = caller.session.as_ref().ok_or_else(invalid_request)?;
        if uuid::Uuid::parse_str(&caller.coordinator_id).is_err()
            || scope.session_id != session.session_id
            || scope.agent_id != session.agent_id
            || scope.workspace_id != session.workspace.workspace_id
            || scope.user_id != session.workspace.user_id
        {
            return Err(invalid_request());
        }
        Ok(&caller.coordinator_id)
    }
    pub(super) fn state_context(
        &self,
        auth: Option<pb::NodeAuth>,
        session: Option<pb::SessionContext>,
    ) -> TalosResult<agent_protocol::AgentSessionContext> {
        let auth = node_pb::decode_auth(auth)?;
        self.authorize(&auth)?;
        let session = harness_pb::decode_session_context(
            session,
            &self.config.read().expect("Worker config lock"),
        )?;
        self.harness(&session.agent_id)?;
        Ok(session)
    }
    pub(super) fn read_execution_state(
        &self,
        request: pb::ReadSessionExecutionStateRequest,
    ) -> TalosResult<agent_protocol::SessionExecutionState> {
        let session = self.state_context(request.auth, request.params)?;
        self.coordinator_id(request.caller.as_ref(), &session)?;
        self.session_gate.read(&session)
    }
    pub(super) fn commit_execution_state(
        &self,
        request: pb::CommitSessionExecutionStateRequest,
    ) -> TalosResult<()> {
        let initialize_local = matches!(
            self.authorize(&node_pb::decode_auth(request.auth.clone())?)?
                .owner,
            AuthorizationOwner::Host
        );
        let params = request.params.ok_or_else(invalid_request)?;
        let session = self.state_context(request.auth, params.session)?;
        let peer = self.coordinator_id(request.caller.as_ref(), &session)?;
        let identity = params
            .identity
            .map(|identity| talos_types::AgentTurnRef::new(identity.session_id, identity.turn_id));
        if identity.as_ref().is_some_and(|identity| {
            identity.session_id != session.session_id || identity.turn_id.is_empty()
        }) {
            return Err(invalid_request());
        }
        let history_cursor = execution_state_pb::decode_history_cursor(
            params.history_cursor.ok_or_else(invalid_request)?,
        )?;
        let visibility = match pb::SessionStateVisibility::try_from(params.visibility) {
            Ok(pb::SessionStateVisibility::Local) => SessionStateVisibility::Local,
            Ok(pb::SessionStateVisibility::Shared) => SessionStateVisibility::Shared,
            _ => return Err(invalid_request()),
        };
        self.session_gate.commit(
            &session,
            peer,
            identity.as_ref(),
            history_cursor,
            visibility,
            if initialize_local {
                super::session_gate::HistoryBootstrap::TrustedLocal
            } else {
                super::session_gate::HistoryBootstrap::Forbidden
            },
        )
    }
}
