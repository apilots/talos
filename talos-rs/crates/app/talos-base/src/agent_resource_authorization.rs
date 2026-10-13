//! Authentication and original persisted binding checks shared by both carriers.
use super::*;
use agent_protocol::NodeAuth;

#[derive(Clone)]
pub(super) enum ResourceAuthority {
    Lease,
    Host(NodeAuth),
    Invocation {
        computer_id: String,
        caller: pb::CoordinatorCallContext,
    },
}
impl NodeSessionDataRpc {
    pub(super) fn should_route(&self) -> bool {
        matches!(self.authority, ResourceAuthority::Lease)
    }

    pub(in crate::agent_node_rpc) fn with_routes(mut self, routes: ResourceRoutes) -> Self {
        self.routes = Some(routes);
        self
    }
    pub(crate) fn with_host_auth(mut self, auth: NodeAuth) -> Self {
        self.authority = ResourceAuthority::Host(auth);
        self.routes = None;
        self
    }
    pub(crate) fn for_invocation(
        &self,
        computer_id: String,
        caller: pb::CoordinatorCallContext,
    ) -> Self {
        let mut data = self.clone();
        data.authority = ResourceAuthority::Invocation {
            computer_id,
            caller,
        };
        data.routes = None;
        data
    }
    pub(super) async fn authorize(
        &self,
        auth: Option<pb::NodeAuth>,
        session_id: &str,
        caller: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<SessionRecord> {
        let auth = node_pb::decode_auth(auth)?;
        match &self.authority {
            ResourceAuthority::Lease => {
                self.nodes.authenticate_lease(&auth).await?;
                let session = self.session(session_id).await?;
                self.nodes.authorize_session_lease(&auth, &session).await?;
                Ok(session)
            }
            ResourceAuthority::Host(expected) => {
                use sha2::{Digest, Sha256};
                use subtle::ConstantTimeEq;
                let expected_hash: [u8; 32] = Sha256::digest(expected.token.as_bytes()).into();
                let supplied_hash: [u8; 32] = Sha256::digest(auth.token.as_bytes()).into();
                if expected.node_id != auth.node_id
                    || !bool::from(expected_hash.ct_eq(&supplied_hash))
                {
                    return Err(denied());
                }
                self.authorize_scope(&auth.node_id, caller.ok_or_else(denied)?, session_id)
                    .await
            }
            ResourceAuthority::Invocation {
                computer_id,
                caller,
            } => self.authorize_scope(computer_id, caller, session_id).await,
        }
    }
    pub(super) async fn authorize_owner(
        &self,
        auth: &NodeAuth,
        supplied: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<String> {
        if matches!(self.authority, ResourceAuthority::Lease) {
            return self.nodes.authorize_lease_owner(auth).await;
        }
        let caller = match &self.authority {
            ResourceAuthority::Invocation { caller, .. } => caller,
            _ => supplied.ok_or_else(denied)?,
        };
        let session_id = &caller.session.as_ref().ok_or_else(denied)?.session_id;
        Ok(self
            .authorize(
                Some(node_pb::encode_auth(auth.clone())),
                session_id,
                Some(caller),
            )
            .await?
            .user_id)
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
        caller: &pb::CoordinatorCallContext,
        session_id: &str,
    ) -> TalosResult<SessionRecord> {
        let scope = caller.session.as_ref().ok_or_else(denied)?;
        let session = self.session(session_id).await?;
        let namespace = self
            .sessions
            .get_workspace_user_id(session_id)
            .await?
            .unwrap_or_else(|| session.user_id.clone());
        if scope.session_id != session_id
            || scope.agent_id != session.agent_id
            || scope.user_id != namespace
            || scope.workspace_id != session.workspace_id
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
