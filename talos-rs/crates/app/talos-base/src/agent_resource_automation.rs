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
