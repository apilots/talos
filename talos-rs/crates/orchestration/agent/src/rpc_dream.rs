use super::*;

impl AgentHarnessRpcAdapter {
    pub(super) async fn serve_dream_memory(
        &self,
        request: Request<Streaming<pb::FormDreamMemoryRequest>>,
    ) -> Result<Response<RpcStream<pb::FormDreamMemoryResponse>>, Status> {
        let mut inbound = request.into_inner();
        let first = tokio::time::timeout(Duration::from_secs(30), inbound.message())
            .await
            .map_err(|_| {
                status_from_error(TalosError::new(
                    ErrorCode::Timeout,
                    "Dream Memory context timed out",
                ))
            })??
            .ok_or_else(|| status_from_error(invalid_request()))?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::form_dream_memory_request::Body::Context(context)) = first.body else {
            return Err(status_from_error(invalid_request()));
        };
        let context = self.memory_context(Some(node_pb::encode_auth(auth)), Some(context))?;
        let harness = self.harness(&context.agent_id).map_err(status_from_error)?;
        let adapter = self.clone();
        let (tx, rx) = mpsc::channel(2);
        let model_tx = tx.clone();
        let model_lease = lease.clone();
        let inbound = Arc::new(tokio::sync::Mutex::new(inbound));
        let model_call: agent_protocol::DreamMemoryModelCall = Arc::new(move |messages| {
            let tx = model_tx.clone();
            let lease = model_lease.clone();
            let adapter = adapter.clone();
            let inbound = inbound.clone();
            Box::pin(async move {
                let mut inbound = inbound.lock().await;
                let prompt = agent_protocol::dream_pb::encode_prompt(messages)?;
                lease
                    .send(
                        &tx,
                        Ok(pb::FormDreamMemoryResponse {
                            result: Some(pb::form_dream_memory_response::Result::ModelPrompt(
                                prompt,
                            )),
                        }),
                    )
                    .await
                    .map_err(|_| unauthenticated())?;
                let frame = tokio::time::timeout(Duration::from_secs(300), inbound.message())
                    .await
                    .map_err(|_| {
                        TalosError::new(ErrorCode::Timeout, "Dream Memory model reply timed out")
                    })?
                    .map_err(transport_error)?
                    .ok_or_else(invalid_request)?;
                let auth = node_pb::decode_auth(frame.auth)?;
                let current = adapter.authorize(&auth)?;
                if !bool::from(current.token_hash.ct_eq(&lease.token_hash)) {
                    return Err(unauthenticated());
                }
                match frame.body.ok_or_else(invalid_request)? {
                    pb::form_dream_memory_request::Body::ModelReply(reply) => {
                        agent_protocol::dream_pb::decode_reply(reply)
                    }
                    pb::form_dream_memory_request::Body::ModelFailure(error) => {
                        Err(error_pb::decode_failure(error)?)
                    }
                    _ => Err(invalid_request()),
                }
            })
        });
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = lease.ended() => return,
                _ = tx.closed() => return,
                result = harness.form_dream_memory(context, model_call) => result,
            };
            let result = match result {
                Ok(covers_until_seq) => {
                    pb::form_dream_memory_response::Result::Formed(pb::DreamMemoryFormation {
                        covers_until_seq,
                    })
                }
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
