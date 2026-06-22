use talos_application_service::{Actor, TimelineSubscription};
use talos_error::TalosResult;
use talos_types::{
    InterruptTurnRequest, InterruptTurnResponse, RecoverTimelineRequest, RetryTurnRequest,
    RetryTurnResponse, StartTurnRequest, StartTurnResponse, SubmitToolResultRequest,
    SubmitToolResultResponse, SubscribeTimelineRequest, TimelineRecoveryResponse,
};
use tracing::info;

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
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
        self.application_service
            .start_turn(actor, request, idempotency_key)
            .await
    }

    pub(crate) async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<RetryTurnResponse> {
        self.application_service.retry_turn(actor, request).await
    }

    pub(crate) async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.application_service
            .interrupt_turn(actor, request)
            .await
    }

    pub(crate) async fn submit_tool_result(
        &self,
        actor: &Actor,
        request: SubmitToolResultRequest,
    ) -> TalosResult<SubmitToolResultResponse> {
        self.application_service
            .submit_tool_result(actor, request)
            .await
    }

    pub(crate) async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscription> {
        self.application_service
            .subscribe_timeline(actor, request)
            .await
    }

    pub(crate) async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse> {
        self.application_service
            .recover_timeline(actor, request)
            .await
    }
}
