use super::*;

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
    pub(super) async fn read_execution_state(
        &self,
        request: pb::ReadSessionExecutionStateRequest,
    ) -> TalosResult<agent_protocol::SessionExecutionState> {
        let session = self.state_context(request.auth, request.params)?;
        self.coordinator_id(request.caller.as_ref(), &session)?;
        self.durable_state(&session).await
    }
    pub(super) async fn durable_state(
        &self,
        session: &agent_protocol::AgentSessionContext,
    ) -> TalosResult<agent_protocol::SessionExecutionState> {
        let mut marker = self.session_gate.read(session)?;
        if let Some(mut state) = self
            .harness(&session.agent_id)?
            .read_session_execution_state(session.clone())
            .await?
        {
            if state.active_turn.is_none() {
                if let Some(identity) = marker.active_turn.clone() {
                    if state.history_cursor.completed_turn_id.as_deref()
                        == Some(identity.turn_id.as_str())
                    {
                        self.session_gate.settled(
                            session,
                            &identity,
                            state.history_cursor.clone(),
                        )?;
                        self.session_owner.release_turn(&identity);
                        marker = self.session_gate.read(session)?;
                    } else if !self
                        .active
                        .lock()
                        .expect("Agent active lock")
                        .contains_key(&(
                            session.agent_id.clone(),
                            identity.session_id.clone(),
                            identity.turn_id.clone(),
                        ))
                        && !self.session_owner.holds_turn(&identity)
                    {
                        self.session_gate.reconcile_unadmitted(
                            session,
                            &identity,
                            state.history_cursor.clone(),
                        )?;
                        marker = self.session_gate.read(session)?;
                    }
                }
            }
            if state.active_turn.is_none()
                && marker.active_turn.is_none()
                && marker.coordinator_id.is_some()
                && marker.history_cursor != state.history_cursor
            {
                self.session_gate
                    .sync_idle_snapshot(session, state.history_cursor.clone())?;
                marker = self.session_gate.read(session)?;
            }
            state.coordinator_id = marker.coordinator_id;
            state.handoff_ready = state.active_turn.is_none() && marker.handoff_ready;
            if state.active_turn.is_none() && marker.active_turn.is_some() {
                state.active_turn = marker.active_turn;
                state.handoff_ready = false;
            }
            Ok(state)
        } else {
            Ok(marker)
        }
    }

    pub(super) async fn already_cleared(
        &self,
        session: &agent_protocol::AgentSessionContext,
        expected: &talos_types::SessionHistoryCursor,
        harness: &Arc<dyn AgentHarness>,
    ) -> TalosResult<bool> {
        Ok(harness
            .read_session_execution_state(session.clone())
            .await?
            .is_some_and(|state| {
                state.active_turn.is_none()
                    && state.history_cursor.history_generation
                        == expected.history_generation.saturating_add(1)
                    && state.history_cursor.completed_turn_id.is_none()
                    && state.history_cursor.completed_event_sequence == 0
            }))
    }

    pub(super) async fn finish_clear(
        &self,
        session: &agent_protocol::AgentSessionContext,
        harness: &Arc<dyn AgentHarness>,
    ) -> TalosResult<()> {
        if let Some(state) = harness
            .read_session_execution_state(session.clone())
            .await?
        {
            self.session_gate
                .reset_cursor(session, state.history_cursor)?;
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
    fn read_execution_payload(
        &self,
        mut params: agent_protocol::AgentExecutionPayloadParams,
    ) -> crate::AgentFuture<'_, TalosResult<agent_protocol::AgentExecutionPayloadResult>> {
        Box::pin(async move {
            params.session = self.session(params.session)?;
            self.adapter
                .harness(&self.agent_id)?
                .read_execution_payload(params)
                .await
        })
    }
    fn read_session_events(
        &self,
        mut params: agent_protocol::AgentSessionEventsParams,
    ) -> crate::AgentFuture<'_, TalosResult<agent_protocol::AgentSessionEventsResult>> {
        Box::pin(async move {
            params.session = self.session(params.session)?;
            self.adapter
                .harness(&self.agent_id)?
                .read_session_events(params)
                .await
        })
    }

    fn read_session_execution_state(
        &self,
        session: agent_protocol::AgentSessionContext,
    ) -> crate::AgentFuture<'_, TalosResult<Option<agent_protocol::SessionExecutionState>>> {
        Box::pin(async move {
            Ok(Some(
                self.adapter.durable_state(&self.session(session)?).await?,
            ))
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
            let harness = self.adapter.harness(&session.agent_id)?;
            if self
                .adapter
                .already_cleared(&session, &expected, &harness)
                .await?
            {
                return self.adapter.finish_clear(&session, &harness).await;
            }
            let (_ownership, _reservation, harness) = self
                .adapter
                .prepare_maintenance(&session, &self.coordinator_id, &expected)
                .await?;
            harness.clear_session(session.clone(), expected).await?;
            self.adapter.finish_clear(&session, &harness).await
        })
    }
    fn retire_session(
        &self,
        session: agent_protocol::AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> crate::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            let session = self.session(session)?;
            let (_ownership, _reservation, harness) = self
                .adapter
                .prepare_maintenance(&session, &self.coordinator_id, &expected)
                .await?;
            harness.retire_session(session.clone(), expected).await?;
            self.adapter.finish_clear(&session, &harness).await
        })
    }
    fn close_session(
        &self,
        session: agent_protocol::AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> crate::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            let session = self.session(session)?;
            let (_ownership, _reservation, harness) = self
                .adapter
                .prepare_maintenance(&session, &self.coordinator_id, &expected)
                .await?;
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
        mut params: agent_protocol::DreamMemoryParams,
    ) -> crate::AgentFuture<'_, TalosResult<agent_protocol::DreamMemoryResult>> {
        Box::pin(async move {
            let session = self.session(params.session)?;
            params.session = session.clone();
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
            tokio::select! { biased; _ = lease.ended() => Err(unauthenticated()), result = harness.form_dream_memory(params) => result }
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
