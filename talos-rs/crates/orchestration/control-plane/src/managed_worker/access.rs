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
            Self::Request(actor) => ensure_managed_worker_access(actor),
            Self::Dispatch(owner) if managed.auth.has_verified_managed_worker_access(owner) => {
                Ok(())
            }
            Self::Dispatch(_) => Err(managed_worker_access_denied()),
        }
    }
}

impl AgentNodeService {
    /// Wait for qualified background work and retain the existing activity admission lock.
    /// Dispatch authority never changes a manually stopped Worker's intent.
    pub async fn acquire_managed_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<Option<tokio::sync::OwnedRwLockReadGuard<()>>> {
        if node_id == "embedded" {
            return Ok(None);
        }
        if self.store.managed_worker_by_node(node_id).await?.is_none() {
            return Ok(None);
        }
        let generation = self
            .wait_managed_worker(
                ManagedCaller::Dispatch(owner),
                node_id,
                Duration::from_secs(self.managed_runtime()?.policy.start_timeout_seconds.into()),
            )
            .await?;
        let admission = self.admission.for_node(node_id).read_owned().await;
        let current = self
            .store
            .managed_worker_by_node(node_id)
            .await?
            .ok_or_else(not_found)?;
        ManagedCaller::Dispatch(owner).authorize(self.managed_runtime()?)?;
        ensure_owner(&current, owner)?;
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

    pub async fn managed_conversation_node(
        &self,
        actor: &Actor,
        request: crate::ManagedConversationBinding<'_>,
    ) -> TalosResult<Option<String>> {
        let Some(record) = self
            .store
            .managed_worker_by_conversation(request.session_id)
            .await?
        else {
            return Ok(None);
        };
        ensure_managed_worker_access(actor)?;
        if record.owner_user_id != actor.user_id {
            return Err(not_found());
        }
        if request.project_id.is_some()
            || request.node_id.is_some_and(|node| node != record.node_id)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "The Talo conversation belongs to its Talo and cannot use a Project.",
            ));
        }
        Ok(Some(record.node_id))
    }

    pub async fn ensure_managed_node_access(
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

    pub async fn authorize_session_dispatch(
        &self,
        owner: &str,
        node_id: &str,
    ) -> TalosResult<talos_types::SessionKind> {
        if node_id != "embedded" && self.is_managed_worker_node(node_id).await? {
            ManagedCaller::Dispatch(owner).authorize(self.managed_runtime()?)?;
            return Ok(talos_types::SessionKind::Talo);
        }
        Ok(talos_types::SessionKind::Standard)
    }
}

pub use talos_auth::ensure_managed_worker_access;

fn managed_worker_access_denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Talo requires a Cloud administrator or a Max account.",
    )
}
