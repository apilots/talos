use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    MessageQueue, MessageQueueAddRequest, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueUpdateRequest,
    StartTurnResponse,
};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn add_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        self.service.add_queued_message(actor, request).await
    }

    pub(crate) async fn list_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        self.service.list_message_queue(actor, request).await
    }

    pub(crate) async fn update_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        self.service.update_queued_message(actor, request).await
    }

    pub(crate) async fn delete_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        self.service.delete_queued_message(actor, request).await
    }

    pub(crate) async fn reorder_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        self.service.reorder_message_queue(actor, request).await
    }

    pub(crate) async fn start_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse> {
        self.service.start_queued_message(actor, request).await
    }
}
