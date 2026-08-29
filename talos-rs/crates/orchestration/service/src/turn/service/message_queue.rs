use super::*;

impl TurnService {
    pub(crate) async fn try_dispatch_next(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<MessageCommandResult>> {
        if !self.can_auto_dispatch(session_id).await? {
            return Ok(None);
        }
        let Some(reservation) = self.message_queue.reserve_front(session_id)? else {
            return Ok(None);
        };
        let session = self.sessions.get_session(session_id).await?.ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("session {session_id}"),
            )
        });
        let session = match session {
            Ok(session) => session,
            Err(error) => {
                self.message_queue.release(reservation)?;
                return Err(error);
            }
        };
        match self.submit_queue_reservation(session, reservation).await {
            Ok(result) => Ok(Some(result)),
            Err(error) if error.is_code(talos_error::ErrorCode::ActiveTurnRunning) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn can_auto_dispatch(&self, session_id: &str) -> TalosResult<bool> {
        let Some(active_turn) = self.event_service.get_active_turn(session_id).await? else {
            return Ok(true);
        };
        match active_turn.status {
            ActiveTurnStatus::Running
            | ActiveTurnStatus::WaitingUserAction
            | ActiveTurnStatus::Cancelling => Ok(false),
            ActiveTurnStatus::Failed => Ok(true),
            ActiveTurnStatus::Completed => {
                let events = self
                    .event_service
                    .list_turn_events(session_id, &active_turn.turn_id, 1)
                    .await?;
                Ok(!events.last().is_some_and(|event| {
                    matches!(
                        &event.payload,
                        AgentEventPayload::TurnCompleted(completed)
                            if stop_reason_blocks_auto_dispatch(&completed.stop_reason)
                    )
                }))
            }
        }
    }

    pub(crate) async fn start_queued_message(
        &self,
        session: TalosSession,
        message_id: Option<&str>,
    ) -> TalosResult<MessageCommandResult> {
        let session_id = session.context.session_id.clone();
        if matches!(
            self.event_service
                .get_active_turn(&session_id)
                .await?
                .map(|turn| turn.status),
            Some(
                ActiveTurnStatus::Running
                    | ActiveTurnStatus::WaitingUserAction
                    | ActiveTurnStatus::Cancelling
            )
        ) {
            return Err(TalosError::public(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session `{session_id}` already has an active turn"),
            ));
        }
        let reservation = match message_id {
            Some(message_id) => self.message_queue.reserve(&session_id, message_id)?,
            None => self
                .message_queue
                .reserve_front(&session_id)?
                .ok_or_else(|| {
                    TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("message queue is empty for session `{session_id}`"),
                    )
                })?,
        };
        self.submit_queue_reservation(session, reservation).await
    }

    async fn submit_queue_reservation(
        &self,
        session: TalosSession,
        reservation: MessageQueueReservation,
    ) -> TalosResult<MessageCommandResult> {
        let session_id = session.context.session_id.clone();
        let binding = match self.session_binding(&session_id).await {
            Ok(binding) => binding,
            Err(error) => {
                self.message_queue.release(reservation)?;
                return Err(error);
            }
        };
        let input = reservation.input().to_vec();
        let (auto_dispatch_tx, auto_dispatch_rx) = oneshot::channel();
        let result = self
            .submit_turn(
                session,
                TurnSubmission { input, binding },
                None,
                Some(auto_dispatch_rx),
            )
            .await;
        match result {
            Ok(result) => {
                self.message_queue.commit(reservation)?;
                let _ = auto_dispatch_tx.send(());
                Ok(result)
            }
            Err(error) => {
                self.message_queue.release(reservation)?;
                Err(error)
            }
        }
    }

    pub(crate) async fn steer_queued_message(
        &self,
        session: TalosSession,
        message_id: &str,
        turn_id: &str,
    ) -> TalosResult<talos_types::MessageQueue> {
        let session_id = session.context.session_id.clone();
        let reservation = self.message_queue.reserve(&session_id, message_id)?;
        if let Err(error) = validate_active_turn_input(reservation.input()) {
            self.message_queue.release(reservation)?;
            return Err(error);
        }
        let active_turn = self.event_service.get_active_turn(&session_id).await?;
        if !matches!(
            active_turn,
            Some(ref active)
                if active.turn_id == turn_id && active.status == ActiveTurnStatus::Running
        ) {
            self.message_queue.release(reservation)?;
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("active turn `{turn_id}` for session `{session_id}`"),
            ));
        }
        let binding = match self.session_binding(&session_id).await {
            Ok(binding) => binding,
            Err(error) => {
                self.message_queue.release(reservation)?;
                return Err(error);
            }
        };
        let input = reservation.input().to_vec();
        let result = self
            .executor
            .submit_input(AgentDispatchInputParams {
                binding,
                session,
                turn_id: turn_id.to_string(),
                input: AgentDispatchInput::Message { input },
            })
            .await;
        match result {
            Ok(()) => self.message_queue.commit(reservation),
            Err(error) => {
                self.message_queue.release(reservation)?;
                Err(error)
            }
        }
    }

    pub(crate) fn validate_message_queue_input(input: &[TalosInputItem]) -> TalosResult<()> {
        validate_user_input(input)
    }
}
