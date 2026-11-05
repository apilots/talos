//! Automation uses the original protected local/reverse resource carrier.
use super::*;
use agent_protocol::automation_pb;
use talos_tools::AutomationControl;

#[derive(Clone)]
pub struct NodeAutomationControlRpc {
    session_data: NodeSessionDataRpc,
    control: Arc<dyn AutomationControl>,
}

impl NodeAutomationControlRpc {
    pub fn new(session_data: NodeSessionDataRpc, control: Arc<dyn AutomationControl>) -> Self {
        Self {
            session_data,
            control,
        }
    }
}

#[tonic::async_trait]
impl pb::agent_automation_control_rpc_server::AgentAutomationControlRpc
    for NodeAutomationControlRpc
{
    async fn execute_automation(
        &self,
        request: Request<pb::ExecuteAutomationRequest>,
    ) -> Result<Response<pb::ExecuteAutomationResponse>, Status> {
        if let Some(caller) = request
            .get_ref()
            .caller
            .clone()
            .filter(|caller| !caller.connection_id.is_empty())
        {
            if self.session_data.should_route() {
                let routes = self
                    .session_data
                    .routes
                    .as_ref()
                    .ok_or_else(resource_status)?;
                let auth = request.get_ref().auth.clone();
                let mut response = routes
                    .forward(
                        &self.session_data.nodes,
                        auth,
                        caller,
                        Box::pin(futures_util::stream::once(async move {
                            Ok(pb::worker_rpc_call::Request::ExecuteAutomation(Box::new(
                                request.into_inner(),
                            )))
                        })),
                    )
                    .await?;
                use futures_util::StreamExt;
                let value = response
                    .next()
                    .await
                    .transpose()?
                    .ok_or_else(resource_status)?;
                let pb::worker_rpc_result::Response::ExecuteAutomation(value) = value else {
                    return Err(resource_status());
                };
                return Ok(Response::new(*value));
            }
        }
        let request = request.into_inner();
        let result = async {
            let params = automation_pb::decode_model_params(request.params)?;
            self.session_data
                .authorize_agent_caller(request.auth, &params.caller, request.caller.as_ref())
                .await?;
            self.control.execute(params).await
        }
        .await;
        Ok(Response::new(automation_pb::encode_response(result)))
    }
}
