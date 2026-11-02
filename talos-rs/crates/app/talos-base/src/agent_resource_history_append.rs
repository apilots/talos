//! One bounded history append call; no independent recording/upload lifecycle.
use super::*;
use futures_util::StreamExt;
use talos_agent_api::trajectory::SessionHistorySupplement;

impl NodeSessionDataRpc {
    pub(crate) async fn append_history_stream<S>(
        &self,
        stream: S,
    ) -> Result<Response<pb::AppendSessionHistorySupplementResponse>, Status>
    where
        S: Stream<Item = Result<pb::AppendSessionHistorySupplementRequest, Status>>
            + Send
            + 'static,
    {
        let mut stream = Box::pin(stream);
        let first = stream
            .next()
            .await
            .transpose()?
            .ok_or_else(resource_status)?;
        if let Some(caller) = first
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.should_route() {
                let routes = self.routes.as_ref().ok_or_else(resource_status)?;
                let auth = first.auth.clone();
                let requests = tokio_stream::once(Ok(first)).chain(stream).map(|frame| {
                    frame.map(|value| {
                        pb::worker_rpc_call::Request::AppendSessionHistorySupplement(Box::new(
                            value,
                        ))
                    })
                });
                let mut responses = routes
                    .forward(&self.nodes, auth, caller, Box::pin(requests))
                    .await?;
                let value = responses
                    .next()
                    .await
                    .transpose()?
                    .ok_or_else(resource_status)?;
                let pb::worker_rpc_result::Response::AppendSessionHistorySupplement(value) = value
                else {
                    return Err(resource_status());
                };
                return Ok(Response::new(*value));
            }
        }
        let result = async {
            let pb::append_session_history_supplement_request::Body::Header(header) =
                first.body.ok_or_else(invalid_resource_request)?
            else {
                return Err(invalid_resource_request());
            };
            let id = header.identity.ok_or_else(invalid_resource_request)?;
            if header.byte_length == 0 || header.byte_length > 32 * 1024 * 1024 {
                return Err(invalid_resource_request());
            }
            if first
                .caller
                .as_ref()
                .and_then(|caller| caller.turn_id.as_ref())
                .is_some_and(|turn| turn != &id.turn_id)
            {
                return Err(invalid_resource_request());
            }
            self.authorize(first.auth.clone(), &id.session_id, first.caller.as_ref())
                .await?;
            static BUFFER_BUDGET: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> =
                std::sync::OnceLock::new();
            let budget = BUFFER_BUDGET
                .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(32 * 1024 * 1024)));
            let _permit = budget
                .clone()
                .try_acquire_many_owned(header.byte_length as u32)
                .map_err(|_| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Execution history append capacity is busy.",
                    )
                })?;
            let mut bytes = Vec::with_capacity(header.byte_length as usize);
            while let Some(frame) = stream.next().await.transpose().map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Execution history stream is incomplete.",
                    error,
                )
            })? {
                if frame.auth != first.auth || frame.caller != first.caller {
                    return Err(invalid_resource_request());
                }
                let pb::append_session_history_supplement_request::Body::Chunk(chunk) =
                    frame.body.ok_or_else(invalid_resource_request)?
                else {
                    return Err(invalid_resource_request());
                };
                if chunk.is_empty()
                    || chunk.len() > 64 * 1024
                    || bytes.len() + chunk.len() > header.byte_length as usize
                {
                    return Err(invalid_resource_request());
                }
                bytes.extend_from_slice(&chunk);
            }
            if bytes.len() != header.byte_length as usize {
                return Err(invalid_resource_request());
            }
            let supplement: SessionHistorySupplement =
                serde_json::from_slice(&bytes).map_err(|error| {
                    TalosError::internal_with_source("Decode optional execution history.", error)
                })?;
            if supplement.identity.session_id != id.session_id
                || supplement.identity.turn_id != id.turn_id
            {
                return Err(invalid_resource_request());
            }
            self.history.append_supplement(supplement).await
        }
        .await;
        Ok(Response::new(pb::AppendSessionHistorySupplementResponse {
            result: Some(match result {
                Ok(()) => {
                    pb::append_session_history_supplement_response::Result::Appended(pb::Empty {})
                }
                Err(error) => {
                    pb::append_session_history_supplement_response::Result::Failure(failure(error))
                }
            }),
        }))
    }
}
