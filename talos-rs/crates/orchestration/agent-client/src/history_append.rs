//! Invocation-scoped append through the existing Session history capability.
use super::*;
use futures_util::{stream, StreamExt};
use talos_agent_api::trajectory::SessionHistorySupplement;

impl AgentSessionDataClient {
    pub(super) async fn append_history_supplement(
        &self,
        supplement: SessionHistorySupplement,
    ) -> TalosResult<()> {
        let bytes = serde_json::to_vec(&supplement).map_err(|error| {
            TalosError::internal_with_source("Encode optional execution history.", error)
        })?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err(TalosError::new(
                ErrorCode::UploadTooLarge,
                "Execution history supplement exceeds its byte limit.",
            ));
        }
        let auth = Some(node_pb::encode_auth(self.current_auth()));
        let caller = self.caller.clone();
        let header = pb::AppendSessionHistorySupplementRequest {
            auth: auth.clone(),
            caller: caller.clone(),
            body: Some(pb::append_session_history_supplement_request::Body::Header(
                pb::SessionHistorySupplementHeader {
                    identity: Some(pb::TurnRef {
                        session_id: supplement.identity.session_id,
                        turn_id: supplement.identity.turn_id,
                    }),
                    byte_length: bytes.len() as u64,
                },
            )),
        };
        // The original RPC stream owns this buffer. No spawned producer, upload
        // registry or retry can outlive the invocation's bounded optional call.
        let chunks = stream::unfold(
            (bytes, 0usize, auth, caller),
            |(bytes, offset, auth, caller)| async move {
                if offset == bytes.len() {
                    return None;
                }
                let end = (offset + 64 * 1024).min(bytes.len());
                let frame = pb::AppendSessionHistorySupplementRequest {
                    auth: auth.clone(),
                    caller: caller.clone(),
                    body: Some(pb::append_session_history_supplement_request::Body::Chunk(
                        bytes[offset..end].to_vec(),
                    )),
                };
                Some((frame, (bytes, end, auth, caller)))
            },
        );
        let frames = tokio_stream::once(header).chain(chunks);
        let mut request = tonic::Request::new(frames);
        request.set_timeout(Duration::from_secs(2));
        let result =
            pb::agent_session_data_rpc_client::AgentSessionDataRpcClient::new(self.channel.clone())
                .append_session_history_supplement(request)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match result.result.ok_or_else(invalid_response)? {
            pb::append_session_history_supplement_response::Result::Appended(_) => Ok(()),
            pb::append_session_history_supplement_response::Result::Failure(error) => {
                Err(error_pb::decode_failure(error)?)
            }
        }
    }
}
