use talos_agent::{AgentProgressEventReceiver, AgentRunSettlement, AgentRunSettlementReceiver};
use talos_agent_api::{AgentErrorInfo, AgentEventPayload, AgentInterruptOutcome};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{CommitRunningInputAdmissionParams, CommitTerminalSettlementParams};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::error;

use super::{ActiveTimelineStream, AgentExecutionDriver, ExecutionPurpose, RunningInputCommit};

impl AgentExecutionDriver {
    pub(super) async fn drive(
        &self,
        active: ActiveTimelineStream,
        mut events: AgentProgressEventReceiver,
        mut inputs: mpsc::Receiver<RunningInputCommit>,
        settlement: AgentRunSettlementReceiver,
        tail: (i64, u64),
        purpose: ExecutionPurpose,
    ) -> TalosResult<AgentRunSettlement> {
        let (mut last_event_seq, mut last_sequence) = tail;
        let mut pending_input: Option<RunningInputCommit> = None;
        let mut input_open = true;
        let mut deliveries = JoinSet::new();
        loop {
            if let Some(mut input) = pending_input.take() {
                let expected_sequence = input
                    .events
                    .first()
                    .map(|event| event.sequence.saturating_sub(1));
                match expected_sequence {
                    Some(expected_sequence) if expected_sequence == last_sequence => {
                        let result = match self
                            .execution_store
                            .commit_running_input_admission(CommitRunningInputAdmissionParams {
                                events: input.events,
                                expected_event_seq: last_event_seq,
                                command: input.command,
                            })
                            .await
                        {
                            Ok(commit) => {
                                last_event_seq = commit.active_turn.last_event_seq;
                                last_sequence = commit
                                    .events
                                    .last()
                                    .expect("committed input admission contains canonical events")
                                    .sequence;
                                let queue_result = match input.queued_input.take() {
                                    Some(queued_input) => queued_input.commit(),
                                    None => Ok(()),
                                };
                                if let Err(error) = queue_result {
                                    error!(
                                        session_id = %active.identity.session_id,
                                        turn_id = %active.identity.turn_id,
                                        error = %error,
                                        "failed to remove an already persisted queued Agent input"
                                    );
                                }
                                self.event_service.publish_committed(commit.events);
                                deliveries.spawn(input.permit.consume());
                                Ok(())
                            }
                            Err(error) => Err(error),
                        };
                        let _ = input.result_tx.send(result);
                        continue;
                    }
                    Some(expected_sequence) if expected_sequence > last_sequence => {
                        pending_input = Some(input);
                    }
                    _ => {
                        let _ = input.result_tx.send(Err(TalosError::public(
                            ErrorCode::Conflict,
                            "Agent input admission no longer follows the active Turn tail",
                        )));
                        continue;
                    }
                }
            }

            tokio::select! {
                event = events.recv() => {
                    let Some(event) = event else {
                        break;
                    };
                    match self
                        .execution_store
                        .commit_progress_event(event.clone(), last_event_seq)
                        .await
                    {
                        Ok(commit) => {
                            last_event_seq = commit.active_turn.last_event_seq;
                            last_sequence = event.sequence;
                            self.event_service.publish_committed(commit.events);
                        }
                        Err(commit_error) => {
                            error!(
                                session_id = %active.identity.session_id,
                                turn_id = %active.identity.turn_id,
                                error = %commit_error,
                                "canonical Agent progress commit failed"
                            );
                            return self.recover_failed_execution(
                                &active,
                                last_event_seq,
                                &commit_error,
                            )
                            .await;
                        }
                    }
                }
                input = inputs.recv(), if input_open && pending_input.is_none() => {
                    match input {
                        Some(input) => pending_input = Some(input),
                        None => input_open = false,
                    }
                }
                delivery = deliveries.join_next(), if !deliveries.is_empty() => {
                    report_delivery_result(&active, delivery);
                }
            }
        }
        if let Some(input) = pending_input {
            let _ = input.result_tx.send(Err(TalosError::public(
                ErrorCode::Conflict,
                "Agent Turn ended before input admission reached its durable position",
            )));
        }
        while let Some(delivery) = deliveries.join_next().await {
            report_delivery_result(&active, Some(delivery));
        }

        let settlement = match settlement.recv().await {
            Ok(settlement) => settlement,
            Err(settlement_error) => {
                let settlement_error = settlement_error.into_talos_error();
                error!(
                    session_id = %active.identity.session_id,
                    turn_id = %active.identity.turn_id,
                    error = %settlement_error,
                    "Agent Harness did not settle the invocation"
                );
                return self
                    .recover_failed_execution(&active, last_event_seq, &settlement_error)
                    .await;
            }
        };
        let expected_event_seq = last_event_seq;
        let result = match &settlement {
            AgentRunSettlement::WaitingForInput {
                event,
                continuation,
                ..
            } => {
                if purpose.is_compaction() {
                    let error = TalosError::public(
                        ErrorCode::ContextCompactionFailed,
                        "Agent compaction cannot wait for User input",
                    );
                    return self
                        .recover_failed_execution(&active, expected_event_seq, &error)
                        .await;
                }
                self.execution_store
                    .commit_waiting_settlement(
                        event.clone(),
                        continuation.clone(),
                        expected_event_seq,
                    )
                    .await
                    .map(|commit| commit.events)
            }
            AgentRunSettlement::Finished {
                event,
                messages,
                metrics,
            } => {
                if let (false, Some(reflection)) =
                    (purpose.is_compaction(), &self.memory_reflection)
                {
                    reflection.reflect(event, messages).await;
                }
                if let (false, Some(inner_model)) = (purpose.is_compaction(), &self.inner_model) {
                    if let Err(error) = self
                        .sessions
                        .generate_title(inner_model, event, messages)
                        .await
                    {
                        tracing::warn!(session_id = %event.identity.session_id, %error, "failed to generate Session title");
                    }
                }
                self.execution_store
                    .commit_terminal_settlement(CommitTerminalSettlementParams {
                        completed_turn_metrics: (!purpose.is_compaction()
                            && matches!(event.payload, AgentEventPayload::TurnCompleted(_)))
                        .then_some(metrics.clone()),
                        event: event.clone(),
                        expected_event_seq,
                    })
                    .await
                    .map(|record| vec![record])
            }
        };
        match result {
            Ok(records) => {
                let terminal = records.last().is_some_and(|record| {
                    matches!(
                        record.payload,
                        AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
                    )
                });
                self.event_service.publish_committed(records);
                if terminal {
                    let _ = active.terminal_commit_tx.send(true);
                }
                Ok(settlement)
            }
            Err(commit_error) => {
                error!(
                    session_id = %active.identity.session_id,
                    turn_id = %active.identity.turn_id,
                    error = %commit_error,
                    "canonical Agent settlement commit failed"
                );
                self.recover_failed_execution(&active, expected_event_seq, &commit_error)
                    .await
            }
        }
    }

    pub(super) async fn recover_failed_execution(
        &self,
        active: &ActiveTimelineStream,
        expected_event_seq: i64,
        commit_error: &TalosError,
    ) -> TalosResult<AgentRunSettlement> {
        let cleanup = match self
            .coordinator
            .interrupt(talos_agent::AgentInterruptParams {
                agent_id: active.agent_id.clone(),
                identity: active.identity.clone(),
            })
            .await
        {
            Ok(outcome) => outcome,
            Err(interrupt_error)
                if interrupt_error.is_code(talos_error::ErrorCode::ResourceNotFound) =>
            {
                AgentInterruptOutcome::Completed
            }
            Err(interrupt_error) => {
                return Err(interrupt_error);
            }
        };
        let failure = AgentErrorInfo {
            code: commit_error.error_code(),
            message: format!(
                "Agent Turn stopped before canonical settlement: {}",
                commit_error.diagnostic_message()
            ),
            retryable: true,
        };
        let settlement = self
            .settle_recovered_turn(
                active.identity.clone(),
                expected_event_seq,
                failure,
                cleanup,
            )
            .await?;
        let _ = active.terminal_commit_tx.send(true);
        Ok(settlement)
    }
}

fn report_delivery_result(
    active: &ActiveTimelineStream,
    delivery: Option<Result<talos_agent::AgentResult<()>, tokio::task::JoinError>>,
) {
    let error = match delivery {
        Some(Ok(Ok(()))) | None => return,
        Some(Ok(Err(error))) => error.into_talos_error(),
        Some(Err(error)) => {
            TalosError::internal(format!("Agent input delivery task failed: {error}"))
        }
    };
    error!(
        session_id = %active.identity.session_id,
        turn_id = %active.identity.turn_id,
        error = %error,
        "durable Agent input delivery failed"
    );
}
