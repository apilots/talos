use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, RecoverTimelineResponse, SubmitTurnInputRequest,
    SubmitTurnInputResponse,
};
use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    RecoverTimelineRequest, RetryTurnRequest, StartTurnRequest, StartTurnResponse,
    SubscribeTimelineRequest, SubscribeTimelineResponse,
};
use tracing::info;

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<StartTurnResponse> {
        info!(
            session_id = %request.session_id,
            input_items = request.input.content.len(),
            "starting turn through gateway"
        );
        self.service
            .start_turn(actor, source, request, idempotency_key)
            .await
    }

    pub(crate) async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<StartTurnResponse> {
        self.service.retry_turn(actor, request).await
    }

    pub(crate) async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.service.interrupt_turn(actor, request).await
    }

    pub(crate) async fn submit_turn_input(
        &self,
        actor: &Actor,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse> {
        self.service.submit_turn_input(actor, request).await
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
    ) -> TalosResult<RecoverTimelineResponse> {
        self.service.recover_timeline(actor, request).await
    }
}
