//! Cloud eligibility for managed lifecycle requests and autonomous dispatch.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ManagedCaller<'a> {
    Request(&'a Actor),
    Dispatch(&'a str),
}

impl<'a> ManagedCaller<'a> {
    pub(super) fn owner(self) -> &'a str {
        match self {
            Self::Request(actor) => &actor.user_id,
            Self::Dispatch(owner) => owner,
        }
    }

    pub(super) fn authorize(self, managed: &ManagedWorkers) -> TalosResult<()> {
        match self {
            Self::Request(actor) => ensure_managed_worker_actor(actor),
            Self::Dispatch(owner) if managed.auth.has_verified_managed_worker_access(owner) => {
                Ok(())
            }
            Self::Dispatch(_) => Err(managed_worker_access_denied()),
        }
    }
}

impl AgentNodeService {
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
        ensure_managed_worker_actor(actor)?;
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

    pub(crate) async fn ensure_managed_node_access(
        &self,
        actor: &Actor,
        node_id: &str,
    ) -> TalosResult<()> {
        if node_id != "embedded"
            && !actor.can_use_managed_worker()
            && self.is_managed_worker_node(node_id).await?
        {
            return Err(managed_worker_access_denied());
        }
        Ok(())
    }

    pub(crate) async fn ensure_managed_dispatch_access(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<()> {
        if node_id != "embedded" && self.is_managed_worker_node(node_id).await? {
            ManagedCaller::Dispatch(owner).authorize(self.managed_runtime()?)?;
        }
        Ok(())
    }
}

pub(super) fn ensure_managed_worker_actor(actor: &Actor) -> TalosResult<()> {
    if !actor.can_use_managed_worker() {
        return Err(managed_worker_access_denied());
    }
    ensure_management_actor(actor)
}

fn managed_worker_access_denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Talo requires a Cloud administrator or a Max account.",
    )
}
