//! Replace only transport authentication while preserving original request streams.
use super::*;
use std::pin::Pin;
impl AgentExecutionConnection {
    pub(crate) async fn forward(
        self: &Arc<Self>,
        requests: Pin<Box<dyn Stream<Item = Result<Call, Status>> + Send>>,
        timeout_ms: u64,
        caller: Option<pb::CoordinatorCallContext>,
    ) -> Result<RpcStream<Reply>, Status> {
        let mut requests = requests;
        let first = requests
            .next()
            .await
            .transpose()?
            .ok_or_else(|| error_pb::encode_status(&closed()))?;
        let mode = match &first {
            Call::Start(_) | Call::CompactSession(_) | Call::FormDreamMemory(_) => {
                RequestMode::Streaming
            }
            Call::SubmitInput(_) => RequestMode::ControlStreaming,
            Call::Interrupt(_) | Call::Shutdown(_) => RequestMode::ControlUnary,
            _ => RequestMode::Unary,
        };
        let auth = node_pb::encode_auth(self.auth.clone());
        let mut first = first;
        agent_protocol::connection_pb::replace_auth(&mut first, &auth);
        if let Some(caller) = &caller {
            agent_protocol::connection_pb::set_execution_caller(&mut first, caller);
        }
        let rest = requests.map(move |request| {
            request.map(|mut request| {
                agent_protocol::connection_pb::replace_auth(&mut request, &auth);
                request
            })
        });
        // Carrier validation supplies a clean bounded stream. A malformed
        // continuation closes this invocation rather than replaying it.
        let requests = tokio_stream::once(Ok(first)).chain(rest);
        self.invoke_results(requests, timeout_ms, mode, None).await
    }
}
