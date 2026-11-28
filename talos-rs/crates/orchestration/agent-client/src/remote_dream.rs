use super::*;

impl RemoteAgentClient {
    pub(super) fn run_dream_memory(
        &self,
        params: agent_protocol::DreamMemoryParams,
    ) -> AgentFuture<'_, AgentResult<agent_protocol::DreamMemoryResult>> {
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(300), async move {
                self.check_agent(&params.session.agent_id)?;
                let caller = self.execution_caller(&params.session, None, None);
                let mut stream = self
                    .transport
                    .clone()
                    .form_dream_memory(rpc_request(
                        tokio_stream::iter([pb::FormDreamMemoryRequest {
                            caller: Some(caller),
                            auth: Some(node_pb::encode_auth(self.auth.clone())),
                            params: Some(agent_protocol::dream_pb::encode_params(params)),
                        }]),
                        Duration::from_secs(300),
                    ))
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .into_inner();
                let frame = stream
                    .message()
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .ok_or_else(transport_closed)?;
                match frame.result.ok_or_else(invalid_response)? {
                    pb::form_dream_memory_response::Result::Formed(value) => {
                        agent_protocol::dream_pb::decode_result(value)
                    }
                    pb::form_dream_memory_response::Result::Failure(value) => {
                        Err(error_pb::decode_failure(value)?)
                    }
                }
            })
            .await
            .map_err(|_| TalosError::new(ErrorCode::Timeout, "Dream Memory RPC timed out"))?
        })
    }
}
