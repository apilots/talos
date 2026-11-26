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
        let params = request.params.ok_or_else(invalid_request)?;
        let session = self.state_context(request.auth.clone(), params.session)?;
        let peer = self.coordinator_id(request.caller.as_ref(), &session)?;
        let identity = params
            .identity
            .map(|identity| talos_types::AgentTurnRef::new(identity.session_id, identity.turn_id));
        let history_cursor = execution_state_pb::decode_history_cursor(
            params.history_cursor.ok_or_else(invalid_request)?,
        )?;
        let visibility = match pb::SessionStateVisibility::try_from(params.visibility) {
            Ok(pb::SessionStateVisibility::Local) => SessionStateVisibility::Local,
            Ok(pb::SessionStateVisibility::Shared) => SessionStateVisibility::Shared,
            _ => return Err(invalid_request()),
        };
        self.commit_typed(
            &node_pb::decode_auth(request.auth)?,
            agent_protocol::CommitSessionExecutionStateParams {
                session,
                identity,
                history_cursor,
                visibility,
            },
            peer,
        )
    }

    pub(super) fn commit_typed(
        &self,
        auth: &NodeAuth,
        params: agent_protocol::CommitSessionExecutionStateParams,
        peer: &str,
    ) -> TalosResult<()> {
        let initialize_local = matches!(self.authorize(auth)?.owner, AuthorizationOwner::Host);
        if params.identity.as_ref().is_some_and(|identity| {
            identity.session_id != params.session.session_id || identity.turn_id.is_empty()
        }) {
            return Err(invalid_request());
        }
        let _ownership = self.session_owner.pin(&params.session.session_id)?;
        self.session_gate.commit(
            &params.session,
            peer,
            params.identity.as_ref(),
            params.history_cursor,
            params.visibility,
            if initialize_local {
                super::session_gate::HistoryBootstrap::TrustedLocal
            } else {
                super::session_gate::HistoryBootstrap::Forbidden
            },
        )?;
        if params.visibility == SessionStateVisibility::Local {
            if let Some(identity) = params.identity.as_ref() {
                self.session_owner.release_turn(identity);
            }
        }
        Ok(())
    }
}

// Library projection of the same Worker boundary, not a second Harness owner.
pub(super) struct LocalAgentHarness {
    pub(super) adapter: AgentHarnessRpcAdapter,
    pub(super) agent_id: String,
    pub(super) auth: NodeAuth,
    pub(super) coordinator_id: String,
}
impl LocalAgentHarness {
    fn session(
        &self,
        mut session: agent_protocol::AgentSessionContext,
    ) -> TalosResult<agent_protocol::AgentSessionContext> {
        self.adapter.authorize(&self.auth)?;
        self.adapter.harness(&self.agent_id)?;
        if session.agent_id != self.agent_id || session.session_id.is_empty() {
            return Err(invalid_request());
        }
        // Resolve paths at the owning Worker, exactly as the external adapter does.
        session.workspace = self
            .adapter
            .config
            .read()
            .expect("Worker config lock")
            .resolve_workspace(&session.workspace.workspace_id, &session.workspace.user_id)?;
        Ok(session)
    }
}
impl AgentHarness for LocalAgentHarness {
    fn read_session_execution_state(
        &self,
        session: agent_protocol::AgentSessionContext,
    ) -> crate::AgentFuture<'_, TalosResult<Option<agent_protocol::SessionExecutionState>>> {
        Box::pin(async move {
            Ok(Some(
                self.adapter.session_gate.read(&self.session(session)?)?,
            ))
        })
    }
    fn commit_session_execution_state(
        &self,
        mut params: agent_protocol::CommitSessionExecutionStateParams,
    ) -> crate::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            params.session = self.session(params.session)?;
            self.adapter
                .commit_typed(&self.auth, params, &self.coordinator_id)
        })
    }
    fn start(
        &self,
        mut params: agent_protocol::AgentRunParams,
    ) -> crate::AgentFuture<'_, TalosResult<AgentRunExecution>> {
        Box::pin(async move {
            params.validate()?;
            if params.context.turn_id.is_empty() {
                return Err(invalid_request());
            }
            params.context.session = self.session(params.context.session)?;
            self.adapter
                .admit_start(&self.auth, params, &self.coordinator_id)
                .await
        })
    }
    fn compact_session(
        &self,
        mut context: agent_protocol::AgentContext,
    ) -> crate::AgentFuture<'_, TalosResult<AgentRunExecution>> {
        Box::pin(async move {
            if context.turn_id.is_empty() {
                return Err(invalid_request());
            }
            context.session = self.session(context.session)?;
            self.adapter
                .admit_compact(&self.auth, context, &self.coordinator_id)
                .await
        })
    }
    fn submit_input(
        &self,
        params: agent_protocol::AgentInputParams,
    ) -> crate::AgentFuture<'_, TalosResult<agent_protocol::AgentAdmission>> {
        Box::pin(async move {
            if params.agent_id != self.agent_id {
                return Err(invalid_request());
            }
            let lease = self.adapter.authorize(&self.auth)?;
            let admission = self
                .adapter
                .admit_input(&self.auth, params, &self.coordinator_id)
                .await?;
            Ok(agent_protocol::AgentAdmission {
                events: admission.events,
                permit: agent_protocol::AgentAdmissionPermit::new(async move {
                    tokio::select! { biased; _ = lease.ended() => Err(unauthenticated()), result = admission.permit.consume() => result }
                }),
            })
        })
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> crate::AgentFuture<'_, TalosResult<talos_agent_api::AgentInterruptOutcome>> {
        Box::pin(async move {
            if params.agent_id != self.agent_id {
                return Err(invalid_request());
            }
            self.adapter
                .interrupt_typed(&self.auth, params, &self.coordinator_id)
                .await
        })
    }
    fn clear_session(
        &self,
        session: agent_protocol::AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> crate::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            let session = self.session(session)?;
            let (_ownership, _reservation, harness) =
                self.adapter
                    .prepare_maintenance(&session, &self.coordinator_id, &expected)?;
            harness.clear_session(session, expected).await
        })
    }
    fn close_session(
        &self,
        session: agent_protocol::AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> crate::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            let session = self.session(session)?;
            let (_ownership, _reservation, harness) =
                self.adapter
                    .prepare_maintenance(&session, &self.coordinator_id, &expected)?;
            harness.close_session(session, expected).await
        })
    }
    fn read_workspace_memory(
        &self,
        session: agent_protocol::AgentSessionContext,
    ) -> crate::AgentFuture<'_, TalosResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            let session = self.session(session)?;
            self.adapter
                .harness(&self.agent_id)?
                .read_workspace_memory(session)
                .await
        })
    }
    fn update_workspace_memory(
        &self,
        session: agent_protocol::AgentSessionContext,
        update: talos_memory::UserWorkspaceMemoryUpdate,
    ) -> crate::AgentFuture<'_, TalosResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            let session = self.session(session)?;
            self.adapter
                .harness(&self.agent_id)?
                .update_workspace_memory(session, update)
                .await
        })
    }
    fn form_dream_memory(
        &self,
        session: agent_protocol::AgentSessionContext,
        model: agent_protocol::DreamMemoryModelCall,
    ) -> crate::AgentFuture<'_, TalosResult<Option<u64>>> {
        Box::pin(async move {
            let session = self.session(session)?;
            let lease = self.adapter.authorize(&self.auth)?;
            let _reflection = self
                .adapter
                .session_gate
                .reflection(&session, &self.coordinator_id)?;
            let _resources = self
                .adapter
                .bind_resources(&self.auth, &session, None)
                .await?;
            let harness = self.adapter.harness(&self.agent_id)?;
            tokio::select! { biased; _ = lease.ended() => Err(unauthenticated()), result = harness.form_dream_memory(session, model) => result }
        })
    }
    fn shutdown(&self, timeout: Duration) -> crate::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            self.adapter.authorize(&self.auth)?;
            let harness = self.adapter.harness(&self.agent_id)?;
            self.adapter
                .shutdown_harness(&self.agent_id, harness, timeout)
                .await
        })
    }
}
