//! Account-authenticated forwarding to the active Turn's original Service.
use crate::AgentNodeClient;
use agent_protocol::{
    coordinator_commands_pb, ActiveSessionCommand, ActiveSessionReply, ActiveSessionTarget,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

impl AgentNodeClient {
    pub async fn invoke_active_command(
        &self,
        token: &str,
        target: ActiveSessionTarget,
        command: ActiveSessionCommand,
    ) -> TalosResult<ActiveSessionReply> {
        let payload = coordinator_commands_pb::encode_request(target, command)?;
        let mut request = tonic::Request::new(payload);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().map_err(|_| {
                TalosError::new(ErrorCode::InvalidRequest, "Account credential is invalid.")
            })?,
        );
        request.set_timeout(std::time::Duration::from_secs(30));
        let reply = agent_pb::agent_coordinator_rpc_client::AgentCoordinatorRpcClient::new(
            self.channel.clone(),
        )
        .invoke_active_command(request)
        .await
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        coordinator_commands_pb::decode_reply(reply)
    }
}
