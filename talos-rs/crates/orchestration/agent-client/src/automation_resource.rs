//! The existing resource channel carries the restricted model control port.
use std::time::Duration;

use agent_pb as pb;
use agent_protocol::{automation_pb, node_pb};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_tools::{AutomationControl, AutomationControlFuture};
use talos_types::{AutomationControlResult, AutomationModelRequest};

use super::{rpc_request, AgentSessionDataClient};

impl AutomationControl for AgentSessionDataClient {
    fn execute(&self, request: AutomationModelRequest) -> AutomationControlFuture<'_> {
        Box::pin(self.execute_automation(request))
    }
}

impl AgentSessionDataClient {
    async fn execute_automation(
        &self,
        request: AutomationModelRequest,
    ) -> TalosResult<AutomationControlResult> {
        if let Some(scope) = &self.caller {
            let matches = scope.session.as_ref().is_some_and(|session| {
                session.session_id == request.caller.session_id
                    && session.agent_id == request.caller.agent_id
            }) && scope
                .turn_id
                .as_deref()
                .is_none_or(|turn| turn == request.caller.turn_id);
            if !matches {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Automation caller belongs to another invocation",
                ));
            }
        }
        let session_id = request.caller.session_id.clone();
        let params = automation_pb::encode_model_params(request)?;
        let response =
            pb::agent_automation_control_rpc_client::AgentAutomationControlRpcClient::new(
                self.channel.clone(),
            )
            .execute_automation(rpc_request(
                pb::ExecuteAutomationRequest {
                    auth: Some(node_pb::encode_auth(self.current_auth())),
                    params: Some(params),
                    caller: self.caller.clone(),
                },
                Duration::from_secs(10),
            ))
            .await
            .map_err(crate::decode_rpc_status)?
            .into_inner();
        let result = automation_pb::decode_response(response)?;
        if result.snapshot.session_id != session_id {
            return Err(TalosError::internal(
                "Automation RPC response belongs to another Session.",
            ));
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "automation_resource_tests.rs"]
mod tests;
