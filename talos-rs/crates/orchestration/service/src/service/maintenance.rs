//! Session maintenance policy shared by local and remote product entry points.
use super::*;

impl TalosService {
    /// Rebuild a Session's derived view while preserving its execution source,
    /// configuration, business state, settlement receipts and public event identities.
    /// A live local writer excludes repair. Waiting facts remain resumable; unavailable
    /// or expired source history fails closed without replacing the current view.
    pub async fn rebuild_session_view(
        &self,
        actor: &Actor,
        session_id: &str,
    ) -> ServiceResult<SessionInfo> {
        let session = self.access.load_session(actor, session_id).await?;
        let _ownership = self.turns.pin_session(session_id)?;
        let _execution_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .write_owned()
            .await;
        self.sessions
            .ensure_worker_execution_session(session_id)
            .await?;
        self.sessions
            .ensure_execution_product_binding(&session)
            .await?;
        self.turns.rebuild_session_view(&session).await?;
        self.get_session(actor, session_id).await
    }

    /// Run one process-local maintenance operation only while the Session has no Turn owner.
    ///
    /// The reservation blocks new Turn admission for this Session for the operation duration.
    /// It does not persist maintenance state or expose the execution gate to callers.
    pub async fn run_idle_session_maintenance<T, F, Fut>(
        &self,
        session_id: &str,
        operation: F,
    ) -> ServiceResult<Option<T>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ServiceResult<T>>,
    {
        self.turns
            .run_when_session_idle(session_id, operation)
            .await
    }

    /// Qualify idle maintenance before waking a managed Worker and retain activity
    /// through execution and product settlement. Dispatch honors manual-stop intent.
    /// Resolve the saved Workspace and namespace through that Worker before the
    /// operation; product-host filesystem configuration never supplies execution paths.
    pub async fn run_idle_worker_maintenance<T, F, Fut>(
        &self,
        session_id: &str,
        operation: F,
    ) -> ServiceResult<Option<T>>
    where
        F: FnOnce(agent_protocol::AgentSessionContext) -> Fut,
        Fut: std::future::Future<Output = ServiceResult<T>>,
    {
        self.turns
            .run_when_session_idle(session_id, || async {
                self.sessions
                    .ensure_worker_execution_session(session_id)
                    .await?;
                let session = self
                    .sessions
                    .load_record(session_id)
                    .await?
                    .ok_or_else(|| {
                        TalosError::new(
                            talos_error::ErrorCode::SessionNotFound,
                            "Dream Memory Session does not exist.",
                        )
                    })?;
                let _activity = self
                    .nodes
                    .acquire_managed_dispatch(&session.user_id, &session.execution_node_id)
                    .await?;
                self.sessions
                    .ensure_worker_execution_session(session_id)
                    .await?;
                let current = self
                    .sessions
                    .load_record(session_id)
                    .await?
                    .ok_or_else(|| {
                        TalosError::new(
                            talos_error::ErrorCode::SessionNotFound,
                            "Dream Memory Session no longer exists.",
                        )
                    })?;
                if current.execution_node_id != session.execution_node_id
                    || current.agent_id != session.agent_id
                    || current.user_id != session.user_id
                    || current.workspace_id != session.workspace_id
                {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "Dream Memory Session binding changed while waiting for its Worker.",
                    ));
                }
                if self
                    .turns
                    .canonical_active_turn(session_id)
                    .await?
                    .is_some()
                {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::ActiveTurnRunning,
                        "Dream Memory requires an idle Session.",
                    ));
                }
                let context = self.sessions.build_agent_session_context(&current).await?;
                let result = operation(context).await;
                self.nodes
                    .record_managed_activity(&session.execution_node_id)
                    .await?;
                result
            })
            .await
    }
}
