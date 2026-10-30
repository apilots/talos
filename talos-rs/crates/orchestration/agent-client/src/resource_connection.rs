use super::*;

impl AgentExecutionConnection {
    pub(crate) fn validate_resource_caller(
        &self,
        caller: &pb::CoordinatorCallContext,
    ) -> TalosResult<()> {
        let calls = self.calls.lock().expect("Agent connection lock poisoned");
        let scope = calls
            .get(&caller.rpc_call_id)
            .and_then(|pending| pending.scope.as_ref())
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Resource callback has no active invocation",
                )
            })?;
        if scope.session != caller.session
            || scope.turn_id != caller.turn_id
            || scope.project_id != caller.project_id
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Resource callback belongs to another invocation",
            ));
        }
        Ok(())
    }
    /// Forward an original typed resource method on an authenticated caller stream.
    /// The Gateway supplies the verified invocation; no resource endpoint is user-selected.
    pub async fn invoke_resource<S>(
        self: &Arc<Self>,
        requests: S,
        caller: pb::CoordinatorCallContext,
        timeout_ms: u64,
    ) -> Result<crate::AgentResourceResponses, Status>
    where
        S: Stream<Item = Result<Call, Status>> + Send + 'static,
    {
        let mut requests = Box::pin(requests);
        let mut first = requests
            .next()
            .await
            .transpose()?
            .ok_or_else(|| error_pb::encode_status(&closed()))?;
        let mode = if matches!(
            &first,
            Call::SaveToolImage(_) | Call::AppendSessionHistorySupplement(_)
        ) {
            RequestMode::Streaming
        } else {
            RequestMode::Unary
        };
        let auth = node_pb::encode_auth(self.auth.clone());
        agent_protocol::connection_pb::replace_auth(&mut first, &auth);
        let rest = requests.map(move |request| {
            request.map(|mut request| {
                agent_protocol::connection_pb::replace_auth(&mut request, &auth);
                request
            })
        });
        Ok(self
            .invoke_results(
                tokio_stream::once(Ok(first)).chain(rest),
                timeout_ms,
                mode,
                Some(caller),
            )
            .await?
            .stream)
    }
}
