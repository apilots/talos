//! Typed Harness delegation over the existing remote transport.

use super::*;

impl AgentHarness for RemoteAgentClient {
    fn read_execution_payload(
        &self,
        params: agent_protocol::AgentExecutionPayloadParams,
    ) -> AgentFuture<'_, AgentResult<agent_protocol::AgentExecutionPayloadResult>> {
        Box::pin(async move {
            self.check_agent(&params.session.agent_id)?;
            let caller =
                self.execution_caller(&params.session, Some(params.identity.turn_id.clone()), None);
            let request = pb::ReadExecutionPayloadRequest {
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                caller: Some(caller),
                params: Some(pb::ReadExecutionPayloadParams {
                    session: Some(harness_pb::encode_session_context(params.session)),
                    identity: Some(harness_pb::encode_turn_ref(params.identity)),
                    payload_id: params.payload_id,
                    offset: params.offset,
                    limit: params.limit.min(256 * 1024) as u32,
                }),
            };
            let response = rpc_response(
                self.transport
                    .clone()
                    .read_execution_payload(rpc_request(request, RPC_WAIT)),
                RPC_WAIT,
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::read_execution_payload_response::Result::Chunk(value) => {
                    if value.data.len() > 256 * 1024 {
                        return Err(invalid_response());
                    }
                    Ok(agent_protocol::AgentExecutionPayloadResult {
                        data: value.data,
                        next_offset: value.next_offset,
                    })
                }
                pb::read_execution_payload_response::Result::Failure(error) => {
                    Err(error_pb::decode_failure(error)?)
                }
            }
        })
    }

    fn read_session_events(
        &self,
        params: agent_protocol::AgentSessionEventsParams,
    ) -> AgentFuture<'_, AgentResult<agent_protocol::AgentSessionEventsResult>> {
        Box::pin(async move {
            self.check_agent(&params.session.agent_id)?;
            let caller = self.execution_caller(&params.session, None, None);
            let response = rpc_response(
                self.transport.read_session_events(rpc_request(
                    pb::ReadSessionEventsRequest {
                        auth: Some(node_pb::encode_auth(self.auth.clone())),
                        params: Some(harness_pb::encode_session_context(params.session)),
                        caller: Some(caller),
                        after_cursor: params
                            .after_cursor
                            .map(agent_protocol::execution_events_pb::encode_cursor),
                        limit: params.limit.min(agent_protocol::MAX_EXECUTION_PAGE_EVENTS) as u32,
                    },
                    RPC_WAIT,
                )),
                RPC_WAIT,
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::read_session_events_response::Result::Page(page) => {
                    agent_protocol::execution_events_pb::decode_page(page)
                }
                pb::read_session_events_response::Result::Failure(error) => {
                    Err(error_pb::decode_failure(error)?)
                }
            }
        })
    }

    fn read_session_execution_state(
        &self,
        session: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<Option<agent_protocol::SessionExecutionState>>> {
        Box::pin(async move {
            self.check_agent(&session.agent_id)?;
            let caller = self.execution_caller(&session, None, None);
            let response = rpc_response(
                self.transport.read_session_execution_state(rpc_request(
                    pb::ReadSessionExecutionStateRequest {
                        auth: Some(node_pb::encode_auth(self.auth.clone())),
                        params: Some(harness_pb::encode_session_context(session)),
                        caller: Some(caller),
                    },
                    RPC_WAIT,
                )),
                RPC_WAIT,
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::session_execution_state_response::Result::State(state) => {
                    agent_protocol::execution_state_pb::decode_state(state).map(Some)
                }
                pb::session_execution_state_response::Result::Failure(error) => {
                    Err(error_pb::decode_failure(error)?)
                }
            }
        })
    }

    fn start(&self, params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(self.run_start(params))
    }
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        Box::pin(self.run_input(params))
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<AgentInterruptOutcome>> {
        Box::pin(self.run_interrupt(params))
    }
    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(self.run_compact(context))
    }
    fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_session(params, expected_history_cursor, SessionOperation::Clear))
    }
    fn retire_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_session(params, expected_history_cursor, SessionOperation::Retire))
    }
    fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_session(params, expected_history_cursor, SessionOperation::Close))
    }
    fn read_workspace_memory(
        &self,
        context: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            self.check_agent(&context.agent_id)?;
            let caller = self.execution_caller(&context, None, None);
            let mut request = tonic::Request::new(pb::SessionRequest {
                expected_history_cursor: None,
                caller: Some(caller),
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                params: Some(harness_pb::encode_session_context(context)),
            });
            request.set_timeout(Duration::from_secs(10));
            let response = rpc_response(
                self.transport.clone().read_workspace_memory(request),
                Duration::from_secs(10),
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::workspace_memory_response::Result::Snapshot(value) => {
                    agent_protocol::memory_pb::decode_snapshot(value)
                }
                pb::workspace_memory_response::Result::Failure(value) => {
                    Err(error_pb::decode_failure(value)?)
                }
            }
        })
    }

    fn update_workspace_memory(
        &self,
        context: AgentSessionContext,
        update: talos_memory::UserWorkspaceMemoryUpdate,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        Box::pin(async move {
            self.check_agent(&context.agent_id)?;
            let mut request = tonic::Request::new(pb::UpdateWorkspaceMemoryRequest {
                auth: Some(node_pb::encode_auth(self.auth.clone())),
                params: Some(pb::UpdateWorkspaceMemoryParams {
                    context: Some(harness_pb::encode_session_context(context)),
                    update: Some(agent_protocol::memory_pb::encode_update(update)),
                }),
            });
            request.set_timeout(Duration::from_secs(10));
            let response = rpc_response(
                self.transport.clone().update_workspace_memory(request),
                Duration::from_secs(10),
            )
            .await?
            .into_inner();
            match response.result.ok_or_else(invalid_response)? {
                pb::workspace_memory_response::Result::Snapshot(value) => {
                    agent_protocol::memory_pb::decode_snapshot(value)
                }
                pb::workspace_memory_response::Result::Failure(value) => {
                    Err(error_pb::decode_failure(value)?)
                }
            }
        })
    }

    fn form_dream_memory(
        &self,
        context: AgentSessionContext,
        model_call: agent_protocol::DreamMemoryModelCall,
    ) -> AgentFuture<'_, AgentResult<Option<u64>>> {
        self.run_dream_memory(context, model_call)
    }

    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(self.run_shutdown(timeout))
    }
}
