use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::SessionRecord;

use super::SessionService;

pub(super) struct SessionExecutionOwnership {
    peer_id: Option<String>,
    product_owner: talos_types::NodeProductOwner,
}
impl Default for SessionExecutionOwnership {
    fn default() -> Self {
        Self {
            peer_id: None,
            product_owner: talos_types::NodeProductOwner::Server,
        }
    }
}

impl SessionService {
    pub(crate) fn set_product_owner(&self, owner: talos_types::NodeProductOwner) {
        self.ownership
            .write()
            .expect("Session execution ownership lock")
            .product_owner = owner;
    }

    pub(crate) async fn supports_worker_execution(&self, session_id: &str) -> TalosResult<bool> {
        match self.ensure_execution_supported(session_id).await {
            Ok(()) => Ok(true),
            Err(error) if error.is_code(ErrorCode::InvalidRequest) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(crate) async fn ensure_product_history_access(
        &self,
        actor: &crate::Actor,
        session_id: &str,
    ) -> TalosResult<()> {
        if actor.request_origin == talos_auth::ProductRequestOrigin::RemoteProduct
            && !self.supports_worker_execution(session_id).await?
        {
            return Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Session was not found.",
            ));
        }
        Ok(())
    }

    pub(crate) async fn product_history_generation(&self, session_id: &str) -> TalosResult<u64> {
        Ok(self
            .session_store
            .get_history_cursor(session_id)
            .await?
            .unwrap_or_default()
            .history_generation)
    }

    pub(crate) async fn ensure_execution_supported(&self, session_id: &str) -> TalosResult<()> {
        self.session_store
            .ensure_worker_execution_session(session_id)
            .await?;
        Ok(())
    }

    pub(crate) async fn ensure_execution_product_binding(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<()> {
        if record.execution_node_id == "embedded" {
            return Ok(());
        }
        let owner = self
            .ownership
            .read()
            .expect("Session execution ownership lock")
            .product_owner;
        if owner == talos_types::NodeProductOwner::Computer {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Personal products execute only their embedded Worker.",
            ));
        }
        let nodes = self.managed_nodes.as_ref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::PermissionDenied,
                "This product has no registered Worker authority.",
            )
        })?;
        nodes
            .ensure_execution_product_owner(&record.execution_node_id, &record.user_id, owner)
            .await
    }

    pub(crate) async fn execution_node_permanently_removed(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<bool> {
        if record.execution_node_id == "embedded"
            || self
                .ownership
                .read()
                .expect("Session execution ownership lock")
                .product_owner
                != talos_types::NodeProductOwner::Server
        {
            return Ok(false);
        }
        match &self.managed_nodes {
            Some(nodes) => {
                nodes
                    .execution_node_permanently_removed(&record.execution_node_id, &record.user_id)
                    .await
            }
            None => Ok(false),
        }
    }

    pub(crate) async fn ensure_worker_idle(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<talos_types::SessionHistoryCursor> {
        self.ensure_execution_product_binding(record).await?;
        let session = self.build_agent_session_context(record).await?;
        let state = self
            .coordinator
            .read_session_execution_state(session)
            .await?;
        if state
            .as_ref()
            .is_some_and(|state| state.active_turn.is_some())
        {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Session has a nonterminal Turn on its Worker.",
            ));
        }
        Ok(state.map_or_else(Default::default, |state| state.history_cursor))
    }

    pub(crate) async fn remote_active_target(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<Option<agent_protocol::ActiveSessionTarget>> {
        let peer = self
            .ownership
            .read()
            .expect("Session ownership lock")
            .peer_id
            .clone();
        let Some(peer) = peer else {
            return Ok(None);
        };
        let session = self.build_agent_session_context(record).await?;
        let Some(state) = self
            .coordinator
            .read_session_execution_state(session)
            .await?
        else {
            return Ok(None);
        };
        let Some(identity) = state.active_turn else {
            return Ok(None);
        };
        let Some(coordinator_id) = state.coordinator_id else {
            return Ok(None);
        };
        if coordinator_id == peer {
            return Ok(None);
        }
        let computer_id = self.execution_computer_id(record).await?;
        Ok(Some(agent_protocol::ActiveSessionTarget {
            computer_id,
            coordinator_id,
            identity,
        }))
    }

    pub(crate) async fn execution_computer_id(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<String> {
        Ok(self
            .session_store
            .get_shared_session_info(&record.id)
            .await?
            .map_or_else(|| record.execution_node_id.clone(), |info| info.node_id))
    }

    pub(crate) fn set_coordinator_id(&self, peer_id: String) {
        self.ownership
            .write()
            .expect("Session ownership lock")
            .peer_id = Some(peer_id);
    }

    /// Product projections and replication acknowledgements cannot grant admission.
    /// A different manager may continue only after the authoritative Worker is idle.
    pub(crate) async fn prepare_execution(
        &self,
        record: &SessionRecord,
    ) -> TalosResult<SessionRecord> {
        self.ensure_execution_supported(&record.id).await?;
        self.ensure_execution_product_binding(record).await?;
        let session = self.build_agent_session_context(record).await?;
        if let Some(state) = self
            .coordinator
            .read_session_execution_state(session)
            .await?
        {
            let manager = self
                .ownership
                .read()
                .expect("Session ownership lock")
                .peer_id
                .clone();
            if state.coordinator_id != manager && state.active_turn.is_some() {
                return Err(TalosError::new(
                    ErrorCode::ActiveTurnRunning,
                    "Session has a nonterminal Turn on its Worker.",
                ));
            }
        }
        Ok(record.clone())
    }
}
