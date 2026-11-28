use super::*;
#[derive(Clone)]
pub struct NodeSubagentControlRpc {
    session_data: NodeSessionDataRpc,
    control: Arc<dyn AgentControl>,
}

impl NodeSubagentControlRpc {
    pub fn new(session_data: NodeSessionDataRpc, control: Arc<dyn AgentControl>) -> Self {
        Self {
            session_data,
            control,
        }
    }

    async fn authorize(
        &self,
        auth: Option<pb::NodeAuth>,
        caller: &talos_types::AgentCaller,
        scope: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<()> {
        self.session_data
            .authorize_agent_caller(auth, caller, scope)
            .await
    }
}

#[tonic::async_trait]
impl pb::agent_subagent_control_rpc_server::AgentSubagentControlRpc for NodeSubagentControlRpc {
    async fn spawn_subagent(
        &self,
        request: Request<pb::SpawnSubagentRequest>,
    ) -> Result<Response<pb::SpawnSubagentResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_spawn_params(request.params)?;
            self.authorize(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.spawn(params).await
        }
        .await;
        Ok(Response::new(pb::SpawnSubagentResponse {
            result: Some(match result {
                Ok(value) => pb::spawn_subagent_response::Result::Spawned(
                    resources_pb::encode_spawn_result(value),
                ),
                Err(error) => pb::spawn_subagent_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn send_subagent_message(
        &self,
        request: Request<pb::SendSubagentMessageRequest>,
    ) -> Result<Response<pb::SendSubagentMessageResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_send_params(request.params)?;
            self.authorize(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.send_message(params).await
        }
        .await;
        Ok(Response::new(pb::SendSubagentMessageResponse {
            result: Some(match result {
                Ok(value) => pb::send_subagent_message_response::Result::Sent(
                    resources_pb::encode_send_result(value),
                ),
                Err(error) => pb::send_subagent_message_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn wait_subagent(
        &self,
        request: Request<pb::WaitSubagentRequest>,
    ) -> Result<Response<pb::SubagentSnapshotResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_wait_params(request.params)?;
            self.authorize(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.wait(params).await
        }
        .await;
        Ok(Response::new(snapshot_response(result)))
    }

    async fn interrupt_subagent(
        &self,
        request: Request<pb::InterruptSubagentRequest>,
    ) -> Result<Response<pb::SubagentSnapshotResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_interrupt_params(request.params)?;
            self.authorize(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.interrupt(params).await
        }
        .await;
        Ok(Response::new(snapshot_response(result)))
    }

    async fn follow_up_subagent(
        &self,
        request: Request<pb::FollowUpSubagentRequest>,
    ) -> Result<Response<pb::SubagentSnapshotResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_follow_up_params(request.params)?;
            self.authorize(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.follow_up(params).await
        }
        .await;
        Ok(Response::new(snapshot_response(result)))
    }

    async fn list_subagents(
        &self,
        request: Request<pb::ListSubagentsRequest>,
    ) -> Result<Response<pb::ListSubagentsResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let params = resources_pb::decode_list_params(request.params)?;
            self.authorize(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.list(params).await
        }
        .await;
        Ok(Response::new(pb::ListSubagentsResponse {
            result: Some(match result {
                Ok(value) => {
                    pb::list_subagents_response::Result::Tree(resources_pb::encode_tree(value))
                }
                Err(error) => pb::list_subagents_response::Result::Failure(failure(error)),
            }),
        }))
    }
}

fn snapshot_response(
    result: TalosResult<talos_types::SubagentSnapshot>,
) -> pb::SubagentSnapshotResponse {
    pb::SubagentSnapshotResponse {
        result: Some(match result {
            Ok(value) => pb::subagent_snapshot_response::Result::Snapshot(
                resources_pb::encode_snapshot(value),
            ),
            Err(error) => pb::subagent_snapshot_response::Result::Failure(failure(error)),
        }),
    }
}
