//! Authentication and original persisted binding checks shared by both carriers.
use super::*;
#[cfg(feature = "server")]
use agent_protocol::NodeAuth;

impl NodeSessionDataRpc {
    pub(crate) async fn authorize_local_session(
        &self,
        context: &agent_protocol::AgentSessionContext,
    ) -> TalosResult<SessionRecord> {
        self.authorize_scope(
            "embedded",
            &context.session_id,
            &context.session_id,
            &context.agent_id,
            &context.workspace.workspace_id,
            &context.workspace.user_id,
        )
        .await
    }

    #[cfg(feature = "server")]
    pub(super) async fn authorize_agent_caller(
        &self,
        auth: Option<pb::NodeAuth>,
        caller: &talos_types::AgentCaller,
        supplied_scope: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<()> {
        self.authorize(auth, &caller.session_id).await?;
        let scope = supplied_scope;
        if let Some(scope) = scope {
            let session = scope.session.as_ref().ok_or_else(denied)?;
            if session.session_id != caller.session_id
                || session.agent_id != caller.agent_id
                || scope
                    .turn_id
                    .as_deref()
                    .is_some_and(|turn| turn != caller.turn_id)
            {
                return Err(denied());
            }
        }
        Ok(())
    }

    #[cfg(feature = "server")]
    pub(super) async fn authorize(
        &self,
        auth: Option<pb::NodeAuth>,
        session_id: &str,
    ) -> TalosResult<SessionRecord> {
        let auth = node_pb::decode_auth(auth)?;
        self.nodes
            .as_ref()
            .ok_or_else(denied)?
            .authenticate_lease(&auth)
            .await?;
        let session = self.session(session_id).await?;
        self.nodes
            .as_ref()
            .ok_or_else(denied)?
            .authorize_session_lease(
                &auth,
                &talos_control_plane::SessionLeaseBinding {
                    execution_node_id: &session.execution_node_id,
                    user_id: &session.user_id,
                },
            )
            .await?;
        Ok(session)
    }
    #[cfg(feature = "server")]
    pub(super) async fn authorize_owner(&self, auth: &NodeAuth) -> TalosResult<String> {
        self.nodes
            .as_ref()
            .ok_or_else(denied)?
            .authorize_lease_owner(auth)
            .await
    }
    async fn session(&self, session_id: &str) -> TalosResult<SessionRecord> {
        self.sessions
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| TalosError::new(ErrorCode::SessionNotFound, "Session was not found"))
    }
    async fn authorize_scope(
        &self,
        computer_id: &str,
        session_id: &str,
        scope_session_id: &str,
        agent_id: &str,
        workspace_id: &str,
        user_id: &str,
    ) -> TalosResult<SessionRecord> {
        let session = self.session(session_id).await?;
        let namespace = self
            .sessions
            .get_workspace_user_id(session_id)
            .await?
            .unwrap_or_else(|| session.user_id.clone());
        if scope_session_id != session_id
            || agent_id != session.agent_id
            || user_id != namespace
            || workspace_id != session.workspace_id
            || computer_id != session.execution_node_id
        {
            return Err(denied());
        }
        Ok(session)
    }
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Resource request does not belong to the authorized Session",
    )
}
