//! Original business resource handlers reached over the authenticated reverse carrier.
use super::*;
use agent_client::{AgentResourceRequests, AgentResourceResponses, AgentResourceRpcHandler};
use futures_util::{future::BoxFuture, StreamExt};
use pb::{
    agent_session_data_rpc_server::AgentSessionDataRpc,
    agent_subagent_control_rpc_server::AgentSubagentControlRpc,
};

pub(crate) struct CoordinatorResources {
    pub(crate) service: Arc<talos_service::TalosService>,
    pub(crate) computer_id: String,
    pub(crate) data: NodeSessionDataRpc,
    pub(crate) images: NodeToolImageRpc,
    pub(crate) control: Arc<dyn AgentControl>,
    pub(crate) files: Arc<talos_service::FileService>,
    pub(crate) auth: Arc<dyn talos_auth::AuthService>,
}
impl AgentResourceRpcHandler for CoordinatorResources {
    fn serves_files(&self) -> bool {
        true
    }
    fn dispatch(
        &self,
        caller: pb::CoordinatorCallContext,
        mut requests: AgentResourceRequests,
    ) -> BoxFuture<'_, Result<AgentResourceResponses, Status>> {
        Box::pin(async move {
            let first = requests.next().await.transpose()?.ok_or_else(invalid)?;
            if let pb::worker_rpc_call::Request::ActiveSessionCommand(request) = first {
                if requests.next().await.transpose()?.is_some() {
                    return Err(invalid());
                }
                let auth = request.auth.as_ref().ok_or_else(invalid)?;
                let identity = self
                    .auth
                    .verify_access_token(&auth.token)
                    .await
                    .map_err(|error| error_pb::encode_status(&error))?;
                if identity.auth_disabled {
                    return Err(invalid());
                }
                let (target, command) =
                    agent_protocol::coordinator_commands_pb::decode_request(*request)
                        .map_err(|error| error_pb::encode_status(&error))?;
                let result = self
                    .service
                    .invoke_active_session_command(
                        &Actor::regular(identity.user_id),
                        target,
                        command,
                    )
                    .await;
                let response = match result {
                    Ok(reply) => agent_protocol::coordinator_commands_pb::encode_reply(reply),
                    Err(error) => pb::ActiveSessionCommandResponse {
                        result: Some(pb::active_session_command_response::Result::Failure(
                            error_pb::encode_failure(&error),
                        )),
                    },
                };
                return Ok(Box::pin(tokio_stream::once(Ok(
                    pb::worker_rpc_result::Response::ActiveSessionCommand(Box::new(response)),
                ))) as AgentResourceResponses);
            }
            if agent_protocol::connection_pb::is_management_peer_call(&first) {
                if requests.next().await.transpose()?.is_some() {
                    return Err(invalid());
                }
                return self.dispatch_peer_file(first).await;
            }
            if agent_protocol::connection_pb::resource_caller(&first) != Some(&caller) {
                return Err(invalid());
            }
            let data = self.data.for_invocation(self.computer_id.clone(), caller);
            let images = self.images.clone().with_authorization(data.clone());
            let control = NodeSubagentControlRpc::new(data.clone(), self.control.clone());
            use pb::worker_rpc_call::Request as Call;
            use pb::worker_rpc_result::Response as Reply;
            macro_rules! unary {
                ($value:expr, $owner:expr, $method:ident, $reply:ident) => {{
                    if requests.next().await.transpose()?.is_some() {
                        return Err(invalid());
                    }
                    let value = $owner.$method(Request::new(*$value)).await?.into_inner();
                    Ok(
                        Box::pin(tokio_stream::once(Ok(Reply::$reply(Box::new(value)))))
                            as AgentResourceResponses,
                    )
                }};
            }
            match first {
                Call::ReadSessionHistory(value) => {
                    unary!(value, data, read_session_history, ReadSessionHistory)
                }
                Call::ReadMemoryProgress(value) => {
                    unary!(value, data, read_memory_progress, ReadMemoryProgress)
                }
                Call::AdvanceMemoryProgress(value) => {
                    unary!(value, data, advance_memory_progress, AdvanceMemoryProgress)
                }
                Call::SpawnSubagent(value) => unary!(value, control, spawn_subagent, SpawnSubagent),
                Call::SendSubagentMessage(value) => {
                    unary!(value, control, send_subagent_message, SendSubagentMessage)
                }
                Call::WaitSubagent(value) => unary!(value, control, wait_subagent, WaitSubagent),
                Call::InterruptSubagent(value) => {
                    unary!(value, control, interrupt_subagent, InterruptSubagent)
                }
                Call::FollowUpSubagent(value) => {
                    unary!(value, control, follow_up_subagent, FollowUpSubagent)
                }
                Call::ListSubagents(value) => unary!(value, control, list_subagents, ListSubagents),
                Call::ResolveAttachments(value) => {
                    if requests.next().await.transpose()?.is_some() {
                        return Err(invalid());
                    }
                    let response = data
                        .resolve_attachments(Request::new(*value))
                        .await?
                        .into_inner();
                    Ok(Box::pin(
                        response.map(|value| {
                            value.map(|value| Reply::ResolveAttachments(Box::new(value)))
                        }),
                    ) as AgentResourceResponses)
                }
                Call::AppendSessionHistorySupplement(value) => {
                    let requests =
                        tokio_stream::once(Ok(*value)).chain(requests.map(|frame| match frame? {
                            Call::AppendSessionHistorySupplement(value) => Ok(*value),
                            _ => Err(invalid()),
                        }));
                    let value = data.append_history_stream(requests).await?.into_inner();
                    Ok(Box::pin(tokio_stream::once(Ok(
                        Reply::AppendSessionHistorySupplement(Box::new(value)),
                    ))) as AgentResourceResponses)
                }
                Call::SaveToolImage(value) => {
                    let requests =
                        tokio_stream::once(Ok(*value)).chain(requests.map(|frame| match frame? {
                            Call::SaveToolImage(value) => Ok(*value),
                            _ => Err(invalid()),
                        }));
                    let value = images.save_stream(requests).await?.into_inner();
                    Ok(
                        Box::pin(tokio_stream::once(Ok(Reply::SaveToolImage(Box::new(
                            value,
                        ))))) as AgentResourceResponses,
                    )
                }
                _ => Err(invalid()),
            }
        })
    }
}
fn invalid() -> Status {
    error_pb::encode_status(&TalosError::new(
        ErrorCode::InvalidRequest,
        "Resource carrier method is invalid",
    ))
}

#[path = "agent_peer_file_source.rs"]
mod peer_files;
