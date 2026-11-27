//! Network service projection over the shared protected Harness adapter.

use super::*;

#[tonic::async_trait]
impl pb::agent_harness_rpc_server::AgentHarnessRpc for AgentHarnessRpcAdapter {
    async fn read_execution_payload(
        &self,
        request: Request<pb::ReadExecutionPayloadRequest>,
    ) -> Result<Response<pb::ReadExecutionPayloadResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let params = request.params.ok_or_else(invalid_request)?;
            let session = self.state_context(request.auth, params.session)?;
            self.coordinator_id(request.caller.as_ref(), &session)?;
            let identity = harness_pb::decode_turn_ref(params.identity)?;
            self.harness(&session.agent_id)?
                .read_execution_payload(agent_protocol::AgentExecutionPayloadParams {
                    session,
                    identity,
                    payload_id: params.payload_id,
                    offset: params.offset,
                    limit: params.limit as usize,
                })
                .await
        }
        .await;
        Ok(Response::new(pb::ReadExecutionPayloadResponse {
            result: Some(match result {
                Ok(value) => {
                    pb::read_execution_payload_response::Result::Chunk(pb::ExecutionPayloadChunk {
                        data: value.data,
                        next_offset: value.next_offset,
                    })
                }
                Err(error) => pb::read_execution_payload_response::Result::Failure(failure(&error)),
            }),
        }))
    }

    async fn read_session_events(
        &self,
        request: Request<pb::ReadSessionEventsRequest>,
    ) -> Result<Response<pb::ReadSessionEventsResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let session = self.state_context(request.auth, request.params)?;
            self.coordinator_id(request.caller.as_ref(), &session)?;
            self.harness(&session.agent_id)?
                .read_session_events(agent_protocol::AgentSessionEventsParams {
                    session,
                    after_cursor: request
                        .after_cursor
                        .map(agent_protocol::execution_events_pb::decode_cursor)
                        .transpose()?,
                    limit: request.limit as usize,
                })
                .await
                .and_then(agent_protocol::execution_events_pb::encode_page)
        }
        .await;
        Ok(Response::new(pb::ReadSessionEventsResponse {
            result: Some(match result {
                Ok(page) => pb::read_session_events_response::Result::Page(page),
                Err(error) => pb::read_session_events_response::Result::Failure(failure(&error)),
            }),
        }))
    }

    type StartStream = RpcStream<pb::RunResponse>;
    type SubmitInputStream = RpcStream<pb::InputResponse>;
    type CompactSessionStream = RpcStream<pb::RunResponse>;
    type FormDreamMemoryStream = RpcStream<pb::FormDreamMemoryResponse>;

    async fn set_registration_admission(
        &self,
        request: Request<pb::SetRegistrationAdmissionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        Ok(Response::new(empty_response(
            self.registration_admission(request.into_inner()).await,
        )))
    }

    async fn read_session_execution_state(
        &self,
        request: Request<pb::ReadSessionExecutionStateRequest>,
    ) -> Result<Response<pb::SessionExecutionStateResponse>, Status> {
        Ok(Response::new(pb::SessionExecutionStateResponse {
            result: Some(
                match self.read_execution_state(request.into_inner()).await {
                    Ok(state) => pb::session_execution_state_response::Result::State(
                        agent_protocol::execution_state_pb::encode_state(state),
                    ),
                    Err(error) => {
                        pb::session_execution_state_response::Result::Failure(failure(&error))
                    }
                },
            ),
        }))
    }

    async fn start(
        &self,
        request: Request<Streaming<pb::StartRequest>>,
    ) -> Result<Response<Self::StartStream>, Status> {
        self.serve_start(Box::pin(request.into_inner())).await
    }

    async fn compact_session(
        &self,
        request: Request<Streaming<pb::CompactSessionRequest>>,
    ) -> Result<Response<Self::CompactSessionStream>, Status> {
        self.serve_compact_session(Box::pin(request.into_inner()))
            .await
    }

    async fn submit_input(
        &self,
        request: Request<Streaming<pb::SubmitInputRequest>>,
    ) -> Result<Response<Self::SubmitInputStream>, Status> {
        self.serve_submit_input(Box::pin(request.into_inner()))
            .await
    }

    async fn interrupt(
        &self,
        request: Request<pb::InterruptRequest>,
    ) -> Result<Response<pb::InterruptResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_interrupt_params(request.params.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing interrupt params",
            ))
        })?)
        .map_err(status_from_error)?;
        let peer = request
            .caller
            .as_ref()
            .map(|caller| caller.coordinator_id.as_str())
            .ok_or_else(|| status_from_error(invalid_request()))?;
        let prepared = self
            .prepare_interrupt(&auth, &params, peer)
            .map_err(status_from_error)?;
        let result = self.finish_interrupt(params, peer, prepared).await;
        Ok(Response::new(pb::InterruptResponse {
            result: Some(match result {
                Ok(value) => pb::interrupt_response::Result::Interrupted(
                    harness_pb::encode_interrupt_outcome(value),
                ),
                Err(error) => pb::interrupt_response::Result::Failure(failure(&error)),
            }),
        }))
    }

    async fn clear_session(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(
            request.params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let peer = self
            .coordinator_id(request.caller.as_ref(), &params)
            .map_err(status_from_error)?;
        let expected_history_cursor = agent_protocol::execution_state_pb::decode_history_cursor(
            request.expected_history_cursor.ok_or_else(|| {
                status_from_error(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Session history cursor is required",
                ))
            })?,
        )
        .map_err(status_from_error)?;
        let harness = self.harness(&params.agent_id).map_err(status_from_error)?;
        if self
            .already_cleared(&params, &expected_history_cursor, &harness)
            .await
            .map_err(status_from_error)?
        {
            return Ok(Response::new(empty_response(
                self.finish_clear(&params, &harness).await,
            )));
        }
        let (_ownership, _reservation, harness) = self
            .prepare_maintenance(&params, peer, &expected_history_cursor)
            .await
            .map_err(status_from_error)?;
        let result = async {
            harness
                .clear_session(params.clone(), expected_history_cursor)
                .await?;
            self.finish_clear(&params, &harness).await
        }
        .await;
        Ok(Response::new(empty_response(result)))
    }

    async fn retire_session(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(
            request.params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let peer = self
            .coordinator_id(request.caller.as_ref(), &params)
            .map_err(status_from_error)?;
        let expected_history_cursor = agent_protocol::execution_state_pb::decode_history_cursor(
            request.expected_history_cursor.ok_or_else(|| {
                status_from_error(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Session history cursor is required",
                ))
            })?,
        )
        .map_err(status_from_error)?;
        let (_ownership, _reservation, harness) = self
            .prepare_maintenance(&params, peer, &expected_history_cursor)
            .await
            .map_err(status_from_error)?;
        let result = async {
            harness
                .retire_session(params.clone(), expected_history_cursor)
                .await?;
            self.finish_clear(&params, &harness).await
        }
        .await;
        Ok(Response::new(empty_response(result)))
    }

    async fn close_session(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = harness_pb::decode_session_context(
            request.params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let peer = self
            .coordinator_id(request.caller.as_ref(), &params)
            .map_err(status_from_error)?;
        let expected_history_cursor = agent_protocol::execution_state_pb::decode_history_cursor(
            request.expected_history_cursor.ok_or_else(|| {
                status_from_error(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Session history cursor is required",
                ))
            })?,
        )
        .map_err(status_from_error)?;
        let (_ownership, _reservation, harness) = self
            .prepare_maintenance(&params, peer, &expected_history_cursor)
            .await
            .map_err(status_from_error)?;
        let result = harness.close_session(params, expected_history_cursor).await;
        Ok(Response::new(empty_response(result)))
    }

    async fn read_workspace_memory(
        &self,
        request: Request<pb::SessionRequest>,
    ) -> Result<Response<pb::WorkspaceMemoryResponse>, Status> {
        let request = request.into_inner();
        let context = self.memory_context(request.auth, request.params)?;
        let harness = self.harness(&context.agent_id).map_err(status_from_error)?;
        let result = harness.read_workspace_memory(context).await;
        Ok(Response::new(pb::WorkspaceMemoryResponse {
            result: Some(
                match result.and_then(agent_protocol::memory_pb::encode_snapshot) {
                    Ok(snapshot) => pb::workspace_memory_response::Result::Snapshot(snapshot),
                    Err(error) => pb::workspace_memory_response::Result::Failure(failure(&error)),
                },
            ),
        }))
    }

    async fn update_workspace_memory(
        &self,
        request: Request<pb::UpdateWorkspaceMemoryRequest>,
    ) -> Result<Response<pb::WorkspaceMemoryResponse>, Status> {
        let request = request.into_inner();
        let params = request.params.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing memory params",
            ))
        })?;
        let context = self.memory_context(request.auth, params.context)?;
        let update = agent_protocol::memory_pb::decode_update(params.update.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing memory update",
            ))
        })?)
        .map_err(status_from_error)?;
        let harness = self.harness(&context.agent_id).map_err(status_from_error)?;
        let result = harness.update_workspace_memory(context, update).await;
        Ok(Response::new(pb::WorkspaceMemoryResponse {
            result: Some(
                match result.and_then(agent_protocol::memory_pb::encode_snapshot) {
                    Ok(snapshot) => pb::workspace_memory_response::Result::Snapshot(snapshot),
                    Err(error) => pb::workspace_memory_response::Result::Failure(failure(&error)),
                },
            ),
        }))
    }

    async fn form_dream_memory(
        &self,
        request: Request<Streaming<pb::FormDreamMemoryRequest>>,
    ) -> Result<Response<Self::FormDreamMemoryStream>, Status> {
        self.serve_dream_memory(Box::pin(request.into_inner()))
            .await
    }

    async fn shutdown(
        &self,
        request: Request<pb::ShutdownRequest>,
    ) -> Result<Response<pb::EmptyResponse>, Status> {
        let request = request.into_inner();
        let auth = node_pb::decode_auth(request.auth).map_err(status_from_error)?;
        self.authorize(&auth).map_err(status_from_error)?;
        let params = request.params.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing shutdown params",
            ))
        })?;
        let harness = self.harness(&params.agent_id).map_err(status_from_error)?;
        let result = self
            .shutdown_harness(
                &params.agent_id,
                harness,
                Duration::from_millis(params.timeout_ms),
            )
            .await;
        Ok(Response::new(empty_response(result)))
    }
}
