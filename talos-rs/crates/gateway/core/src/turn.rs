use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    InterruptTurnRequest, InterruptTurnResponse, RecoverTimelineRequest, RetryTurnRequest,
    RetryTurnResponse, StartTurnRequest, StartTurnResponse, SubmitToolResultRequest,
    SubmitToolResultResponse, SubscribeTimelineRequest, SubscribeTimelineResponse,
    TimelineRecoveryResponse,
};
use tracing::info;

use crate::service_gateway::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<StartTurnResponse> {
        info!(
            session_id = %request.session_id,
            input_items = request.input.len(),
            "starting turn through gateway"
        );
        self.service
            .start_turn(actor, request, idempotency_key)
            .await
    }

    pub(crate) async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<RetryTurnResponse> {
        self.service.retry_turn(actor, request).await
    }

    pub(crate) async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.service.interrupt_turn(actor, request).await
    }

    pub(crate) async fn submit_tool_result(
        &self,
        actor: &Actor,
        request: SubmitToolResultRequest,
    ) -> TalosResult<SubmitToolResultResponse> {
        self.service.submit_tool_result(actor, request).await
    }

    pub(crate) async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        self.service.subscribe_timeline(actor, request).await
    }

    pub(crate) async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.service.recover_timeline(actor, request).await
    }
}
