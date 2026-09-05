use super::*;

impl TalosService {
    /// Adds one message to the process-local queue and attempts idle-session dispatch.
    pub async fn add_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueAddRequest,
    ) -> ServiceResult<MessageQueue> {
        let _session = self.access.load_session(actor, &request.session_id).await?;
        TurnService::validate_message_queue_input(&request.input)?;
        self.message_queue.add(&request.session_id, request.input)?;
        if let Err(error) = self.turns.try_dispatch_next(&request.session_id).await {
            tracing::warn!(
                session_id = %request.session_id,
                error = %error,
                "queued message was retained after automatic dispatch failed"
            );
        }
        self.message_queue.list(&request.session_id)
    }

    /// Returns the complete bounded queue for one authorized Session.
    pub async fn list_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueListRequest,
    ) -> ServiceResult<MessageQueue> {
        let _session = self.access.load_session(actor, &request.session_id).await?;
        self.message_queue.list(&request.session_id)
    }

    /// Replaces one waiting message while preserving its identifier and position.
    pub async fn update_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueUpdateRequest,
    ) -> ServiceResult<MessageQueue> {
        let _session = self.access.load_session(actor, &request.session_id).await?;
        TurnService::validate_message_queue_input(&request.input)?;
        self.message_queue
            .update(&request.session_id, &request.message_id, request.input)
    }

    /// Deletes one waiting message from an authorized Session queue.
    pub async fn delete_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueDeleteRequest,
    ) -> ServiceResult<MessageQueue> {
        let _session = self.access.load_session(actor, &request.session_id).await?;
        self.message_queue
            .delete(&request.session_id, &request.message_id)
    }

    /// Applies one complete queue permutation for an authorized Session.
    pub async fn reorder_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueReorderRequest,
    ) -> ServiceResult<MessageQueue> {
        let _session = self.access.load_session(actor, &request.session_id).await?;
        self.message_queue
            .reorder(&request.session_id, &request.message_ids)
    }

    /// Starts the queue head or one explicitly selected waiting message.
    pub async fn start_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueStartRequest,
    ) -> ServiceResult<StartTurnResponse> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        self.turns
            .start_queued_message(session, request.message_id.as_deref())
            .await
    }
}
