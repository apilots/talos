use super::*;

impl TalosService {
    /// Adds one message to the process-local queue and attempts idle-session dispatch.
    pub async fn add_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueAddRequest,
    ) -> ServiceResult<MessageQueue> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::AddMessage(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Queue(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
        TurnService::validate_message_queue_input(&request.input)?;
        let automation = self.turns.installed_automation();
        let guard = if let Some(a) = automation.as_ref() {
            Some(a.lock(&request.session_id).await)
        } else {
            None
        };
        if let Some(a) = automation.as_ref() {
            a.validate_user_input(&request.session_id, &request.input, None)
                .await?;
        }
        let input = if let Some(a) = automation.as_ref() {
            a.normalize_activation(&session, request.input).await?
        } else {
            request.input
        };
        self.message_queue.add(&request.session_id, input)?;
        drop(guard);
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
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::ListMessages(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Queue(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
        self.message_queue.list(&request.session_id)
    }

    /// Replaces one waiting message while preserving its identifier and position.
    pub async fn update_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueUpdateRequest,
    ) -> ServiceResult<MessageQueue> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::UpdateMessage(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Queue(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
        TurnService::validate_message_queue_input(&request.input)?;
        let automation = self.turns.installed_automation();
        let _guard = if let Some(a) = automation.as_ref() {
            Some(a.lock(&request.session_id).await)
        } else {
            None
        };
        if let Some(a) = automation.as_ref() {
            a.validate_user_input(
                &request.session_id,
                &request.input,
                Some(&request.message_id),
            )
            .await?;
        }
        self.message_queue
            .update(&request.session_id, &request.message_id, request.input)
    }

    /// Deletes one waiting message from an authorized Session queue.
    pub async fn delete_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueDeleteRequest,
    ) -> ServiceResult<MessageQueue> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::DeleteMessage(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Queue(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
        self.message_queue
            .delete(&request.session_id, &request.message_id)
    }

    /// Applies one complete queue permutation for an authorized Session.
    pub async fn reorder_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueReorderRequest,
    ) -> ServiceResult<MessageQueue> {
        let session = self.access.load_session(actor, &request.session_id).await?;
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::ReorderMessages(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Queue(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
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
        if let Some(reply) = self
            .forward_active_command(
                actor,
                &session,
                agent_protocol::ActiveSessionCommand::StartMessage(request.clone()),
            )
            .await?
        {
            let agent_protocol::ActiveSessionReply::Started(reply) = reply else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Current coordinator returned an invalid command result.",
                ));
            };
            return Ok(reply);
        }
        let queue = self.message_queue.list(&request.session_id)?;
        let selected = match request.message_id.as_deref() {
            Some(id) => queue.messages.iter().find(|m| m.id == id),
            None => queue.messages.first(),
        };
        if selected.is_some_and(|m| m.source != talos_types::QueuedMessageSource::User) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Automatic Loop messages are read-only.",
            ));
        }
        self.turns
            .start_queued_message(session, request.message_id.as_deref())
            .await
    }
}
