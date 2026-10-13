use super::*;

impl RemoteAgentClient {
    pub(super) fn run_dream_memory(
        &self,
        context: AgentSessionContext,
        model_call: agent_protocol::DreamMemoryModelCall,
    ) -> AgentFuture<'_, AgentResult<Option<u64>>> {
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(300), async move {
                self.check_agent(&context.agent_id)?;
                let (tx, rx) = mpsc::channel(2);
                let caller = self.execution_caller(&context, None, None);
                tx.send(pb::FormDreamMemoryRequest {
                    caller: Some(caller),
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    body: Some(pb::form_dream_memory_request::Body::Context(
                        harness_pb::encode_session_context(context),
                    )),
                })
                .await
                .map_err(|_| transport_closed())?;
                let mut stream = self
                    .transport
                    .clone()
                    .form_dream_memory(rpc_request(
                        ReceiverStream::new(rx),
                        Duration::from_secs(300),
                    ))
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .into_inner();
                while let Some(frame) = stream.message().await.map_err(crate::decode_rpc_status)? {
                    match frame.result.ok_or_else(invalid_response)? {
                        pb::form_dream_memory_response::Result::ModelPrompt(prompt) => {
                            let reply =
                                match model_call(agent_protocol::dream_pb::decode_prompt(prompt))
                                    .await
                                {
                                    Ok(response) => {
                                        pb::form_dream_memory_request::Body::ModelReply(
                                            agent_protocol::dream_pb::encode_reply(response),
                                        )
                                    }
                                    Err(error) => {
                                        talos_logging::report_error(&error);
                                        pb::form_dream_memory_request::Body::ModelFailure(
                                            error_pb::encode_failure(&error),
                                        )
                                    }
                                };
                            tx.send(pb::FormDreamMemoryRequest {
                                caller: None,
                                auth: Some(node_pb::encode_auth(self.auth.clone())),
                                body: Some(reply),
                            })
                            .await
                            .map_err(|_| transport_closed())?;
                        }
                        pb::form_dream_memory_response::Result::Formed(value) => {
                            return Ok(value.covers_until_seq)
                        }
                        pb::form_dream_memory_response::Result::Failure(value) => {
                            return Err(error_pb::decode_failure(value)?)
                        }
                    }
                }
                Err(transport_closed())
            })
            .await
            .map_err(|_| TalosError::new(ErrorCode::Timeout, "Dream Memory RPC timed out"))?
        })
    }
}
