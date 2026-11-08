use super::*;

impl TurnService {
    pub(crate) async fn try_dispatch_next(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<StartTurnResponse>> {
        let auto_dispatch = self.can_auto_dispatch(session_id).await?;
        let reservation = match self.message_queue.reserve_front(session_id) {
            Ok(Some(reservation)) => reservation,
            Ok(None) => return Ok(None),
            // Another original dispatcher owns this row while admission is in flight.
            Err(error) if error.is_code(talos_error::ErrorCode::MessageQueueBusy) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if !auto_dispatch {
            let allowed = if let talos_types::QueuedMessageSource::Loop { loop_id, .. } =
                reservation.source()
            {
                self.can_dispatch_automation(
                    session_id,
                    &talos_types::AutomationObjectRef::Loop {
                        loop_id: loop_id.clone(),
                    },
                )
                .await
            } else {
                Ok(false)
            };
            match allowed {
                Ok(true) => {}
                result => {
                    self.message_queue.release(reservation)?;
                    return result.map(|_| None);
                }
            }
        }
        let session = self
            .execution
            .sessions
            .load_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("Session {session_id} was not found."),
                )
            });
        let session = match session {
            Ok(session) => session,
            Err(error) => {
                self.message_queue.release(reservation)?;
                return Err(error);
            }
        };
        match self
            .defer_managed_dispatch(&session, super::managed_wake::WakeCandidate::Queue)
            .await
        {
            Ok(false) => {}
            result => {
                self.message_queue.release(reservation)?;
                return result.map(|_| None);
            }
        }
        match self.submit_queue_reservation(session, reservation).await {
            Ok(result) => Ok(Some(result)),
            Err(error) if error.is_code(talos_error::ErrorCode::ActiveTurnRunning) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) async fn can_auto_dispatch(&self, session_id: &str) -> TalosResult<bool> {
        if self.canonical_active_turn(session_id).await?.is_some() {
            return Ok(false);
        }
        let latest_terminal = self
            .execution
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

    pub(super) async fn can_dispatch_automation(
        &self,
        session_id: &str,
        object: &talos_types::AutomationObjectRef,
    ) -> TalosResult<bool> {
        if self.can_auto_dispatch(session_id).await? {
            return Ok(true);
        }
        match self.automation.get() {
            Some(automation) => {
                automation
                    .allows_explicit_dispatch(session_id, object)
                    .await
            }
            None => Ok(false),
        }
    }

    pub(crate) async fn start_queued_message(
        &self,
        session: talos_store::session::SessionRecord,
        message_id: Option<&str>,
    ) -> TalosResult<StartTurnResponse> {
        let session_id = session.id.clone();
        if self.canonical_active_turn(&session_id).await?.is_some() {
            return Err(TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session `{session_id}` already has an active turn"),
            ));
        }
        let cancelled_boundary = if let Some(automation) = self.automation.get() {
            automation.cancelled_dispatch_boundary(&session_id).await?
        } else {
            None
        };
        let reservation = match message_id {
            Some(message_id) => self.message_queue.reserve(&session_id, message_id)?,
            None => self
                .message_queue
                .reserve_front(&session_id)?
                .ok_or_else(|| {
                    TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("message queue is empty for session `{session_id}`"),
                    )
                })?,
        };
        if let (Some(automation), talos_types::QueuedMessageSource::Loop { loop_id, .. }) =
            (self.automation.get(), reservation.source())
        {
            automation.allow_explicit_dispatch(
                &session_id,
                talos_types::AutomationObjectRef::Loop {
                    loop_id: loop_id.clone(),
                },
                cancelled_boundary,
            );
        }
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
        let source = reservation.source().clone();
        let preparation = self.prepare_queued_automation(&session, &reservation).await;
        let (input, origin) = match preparation {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.message_queue.discard(reservation)?;
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Loop is no longer eligible for admission.",
                ));
            }
            Err(error) => {
                self.message_queue.release(reservation)?;
                if let talos_types::QueuedMessageSource::Loop { loop_id, .. } = source {
                    self.automation_service()?
                        .failed_loop_admission(&loop_id, &error)
                        .await?;
                }
                return Err(error);
            }
        };

        let (auto_dispatch_tx, auto_dispatch_rx) = oneshot::channel();
        let result = self
            .submit_turn_owned(
                session,
                TurnSubmission {
                    origin,
                    queue_message_id: Some(reservation.message_id().to_owned()),
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
                if let talos_types::QueuedMessageSource::Loop { loop_id, .. } = source {
                    self.automation_service()?
                        .failed_loop_admission(&loop_id, &error)
                        .await?;
                }
                Err(error)
            }
        }
    }

    async fn prepare_queued_automation(
        &self,
        session: &talos_store::session::SessionRecord,
        reservation: &MessageQueueReservation,
    ) -> TalosResult<Option<(AgentUserInput, Option<talos_types::AutomationTurnOrigin>)>> {
        if let talos_types::QueuedMessageSource::Loop {
            loop_id,
            occurrence_id,
        } = reservation.source()
        {
            return self
                .automation_service()?
                .prepare_loop(session, loop_id, occurrence_id)
                .await
                .map(|value| value.map(|(input, origin)| (input, Some(origin))));
        }
        if let Some(automation) = self.automation.get() {
            automation
                .validate_user_input(
                    &session.id,
                    reservation.input(),
                    Some(reservation.message_id()),
                )
                .await?;
        }
        Ok(Some((reservation.input().clone(), None)))
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
        if reservation.source() != &talos_types::QueuedMessageSource::User {
            self.message_queue.release(reservation)?;
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Automatic Loop messages cannot Steer.",
            ));
        }
        let activation = match talos_types::explicit_automation_mode(reservation.input()) {
            Ok(mode) => mode,
            Err(error) => {
                self.message_queue.release(reservation)?;
                return Err(error);
            }
        };
        if activation.is_some() {
            self.message_queue.release(reservation)?;
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Automation activation messages must wait for their own Turn.",
            ));
        }
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
            return Err(TalosError::new(
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
