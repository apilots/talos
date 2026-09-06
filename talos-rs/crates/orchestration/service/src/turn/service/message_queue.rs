use super::*;

impl TurnService {
    pub(crate) async fn try_dispatch_next(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<StartTurnResponse>> {
        if !self.can_auto_dispatch(session_id).await? {
            return Ok(None);
        }
        let Some(reservation) = self.message_queue.reserve_front(session_id)? else {
            return Ok(None);
        };
        let session = self.sessions.load_record(session_id).await?.ok_or_else(|| {
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
        if self.canonical_active_turn(session_id).await?.is_some() {
            return Ok(false);
        }
        let latest_terminal = self
            .execution_store
            .get_latest_agent_terminal_event(session_id)
            .await?;
        Ok(!latest_terminal.is_some_and(|event| {
            matches!(
                event.payload,
                talos_agent_api::AgentEventPayload::TurnFailed(failed)
                    if failed.error.code == talos_error::ErrorCode::Cancelled
            )
        }))
    }

    pub(crate) async fn start_queued_message(
        &self,
        session: talos_store::session::SessionRecord,
        message_id: Option<&str>,
    ) -> TalosResult<StartTurnResponse> {
        let session_id = session.id.clone();
        if self.canonical_active_turn(&session_id).await?.is_some() {
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
        session: talos_store::session::SessionRecord,
        reservation: MessageQueueReservation,
    ) -> TalosResult<StartTurnResponse> {
        let service = self.clone();
        super::await_admission(async move {
            service
                .submit_queue_reservation_owned(session, reservation)
                .await
        })
        .await
    }

    async fn submit_queue_reservation_owned(
        &self,
        session: talos_store::session::SessionRecord,
        reservation: MessageQueueReservation,
    ) -> TalosResult<StartTurnResponse> {
        let input = reservation.input().clone();
        let (auto_dispatch_tx, auto_dispatch_rx) = oneshot::channel();
        let result = self
            .submit_turn_owned(
                session,
                TurnSubmission {
                    input,
                    retry_of_message_id: None,
                },
                None,
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
        session: talos_store::session::SessionRecord,
        message_id: &str,
        turn_id: &str,
        command: Option<CommandLease>,
    ) -> TalosResult<()> {
        let session_id = session.id.clone();
        let reservation = self.message_queue.reserve(&session_id, message_id)?;
        if let Err(error) = validate_active_turn_input(reservation.input()) {
            self.message_queue.release(reservation)?;
            return Err(error);
        }
        let active_turn = self.canonical_active_turn(&session_id).await?;
        if !matches!(
            active_turn,
            Some(ref active)
                if active.identity.turn_id == turn_id && active.state == ActiveTurnState::Running
        ) {
            self.message_queue.release(reservation)?;
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("active turn `{turn_id}` for session `{session_id}`"),
            ));
        }
        let input = reservation.input().clone();
        let identity = AgentTurnRef::new(&session_id, turn_id);
        let input = crate::turn::completed_agent_user_input(
            turn_id,
            message_id,
            input,
            chrono::Utc::now().timestamp(),
        )?;
        self.execution
            .admit_queued_running_input(
                AgentInputParams {
                    agent_id: session.agent_id,
                    identity,
                    input,
                },
                command,
                self.message_queue.clone(),
                reservation,
            )
            .await?;
        Ok(())
    }

    pub(crate) fn validate_message_queue_input(input: &AgentUserInput) -> TalosResult<()> {
        validate_user_input(input)
    }
}
