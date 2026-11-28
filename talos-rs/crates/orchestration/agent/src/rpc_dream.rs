use super::*;

impl AgentHarnessRpcAdapter {
    pub(super) async fn serve_dream_memory(
        &self,
        request: RpcStream<pb::FormDreamMemoryRequest>,
    ) -> Result<Response<RpcStream<pb::FormDreamMemoryResponse>>, Status> {
        let mut inbound = request;
        let first = tokio::time::timeout(Duration::from_secs(30), inbound.next())
            .await
            .map_err(|_| {
                status_from_error(TalosError::new(
                    ErrorCode::Timeout,
                    "Dream Memory context timed out",
                ))
            })?
            .transpose()?
            .ok_or_else(|| status_from_error(invalid_request()))?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let params = agent_protocol::dream_pb::decode_params(
            first.params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let context = params.session.clone();
        let harness = self.harness(&context.agent_id).map_err(status_from_error)?;
        let peer = self
            .coordinator_id(first.caller.as_ref(), &context)
            .map_err(status_from_error)?;
        let reflection = self
            .session_gate
            .reflection(&context, peer)
            .map_err(status_from_error)?;
        let resource_guard = self
            .bind_resources(&auth, &context, first.caller.as_ref())
            .await
            .map_err(status_from_error)?;
        let (tx, rx) = mpsc::channel(2);
        tokio::spawn(async move {
            let _reflection = reflection;
            let _resources = resource_guard;
            let result = tokio::select! {
                biased;
                _ = lease.ended() => return,
                _ = tx.closed() => return,
                result = harness.form_dream_memory(params) => result,
            };
            let result = match result {
                Ok(result) => pb::form_dream_memory_response::Result::Formed(
                    agent_protocol::dream_pb::encode_result(result),
                ),
                Err(error) => pb::form_dream_memory_response::Result::Failure(failure(&error)),
            };
            let _ = lease
                .send(
                    &tx,
                    Ok(pb::FormDreamMemoryResponse {
                        result: Some(result),
                    }),
                )
                .await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
