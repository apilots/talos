use std::sync::Arc;

use talos_agent_api::{AgentEventPayload, AgentInterruptOutcome, AgentTurnStarted};
use talos_run_control::{
    ActiveRunControl, CancellationMode, RunTaskCompletion, RunTaskError, RunTermination,
    SettlementKind,
};
use tokio::sync::{oneshot, watch};

use super::settlement::{
    cleanup_confirmed, cleanup_error, interrupt_outcome, settle_failure,
    settle_provider_completion, spawn_error,
};
use super::{
    ActiveInvocation, ActiveOperation, Harness, RUN_CANCELLATION_POLICY, RUN_COMPLETION_TIMEOUT,
};
use crate::{
    event_sink::AgentEventPipeline, AgentAdmission, AgentAdmissionPermit, AgentError,
    AgentErrorKind, AgentEventSink, AgentInterruptParams, AgentOperationControl,
    AgentProgressEventReceiver, AgentProvider, AgentResult, AgentRunExecution, AgentRunParams,
    AgentRunSettlement, AgentRunSettlementReceiver, AgentSession, SessionTurnInputReceiver,
    SessionTurnParams,
};

impl<P> Harness<P>
where
    P: AgentProvider,
{
    /// Admit one logical Turn and return its paused execution delivery.
    pub async fn start(&self, params: AgentRunParams) -> AgentResult<AgentRunExecution> {
        params.validate()?;
        self.validate_agent_id(&params.context.session.agent_id)?;
        if params.continuation.is_some() {
            self.validate_continuation_input_capabilities(&params.input)?;
        } else {
            self.validate_fresh_input_capabilities(&params.input)?;
        }
        let identity = params.context.identity();
        let control = Arc::new(ActiveRunControl::new(identity.clone()));
        self.reserve(&identity, control.clone())?;

        let pipeline_result = match params.checkpoint.clone() {
            Some(checkpoint) => AgentEventPipeline::resumed(identity.clone(), checkpoint),
            None => Ok(AgentEventPipeline::fresh(identity.clone())),
        };
        let (pipeline, provider_sink, progress) = match pipeline_result {
            Ok(pipeline) => pipeline,
            Err(error) => {
                self.remove_exact(&identity);
                return Err(error);
            }
        };
        let mut admission_events = Vec::new();
        if params.checkpoint.is_none() {
            match pipeline
                .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
                .await
            {
                Ok(event) => admission_events.push(event),
                Err(error) => {
                    self.remove_exact(&identity);
                    return Err(error);
                }
            }
        }
        match pipeline.record_user_message(params.input.clone()).await {
            Ok(events) => admission_events.extend(events),
            Err(error) => {
                self.remove_exact(&identity);
                return Err(error);
            }
        }

        let (start_tx, start_rx) = oneshot::channel();
        let permit = AgentAdmissionPermit::new(async move {
            start_tx
                .send(())
                .map_err(|_| AgentError::run_not_found("admitted Agent run is no longer active"))
        });
        let (settlement_tx, settlement_rx) = oneshot::channel();
        let harness = self.clone();
        let task = tokio::spawn(async move {
            harness
                .drive_invocation(
                    params,
                    control,
                    pipeline,
                    provider_sink,
                    start_rx,
                    settlement_tx,
                )
                .await;
        });
        drop(task);

        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: admission_events,
                permit,
            },
            events: AgentProgressEventReceiver::new(progress),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        })
    }

    /// Cancel one exact live Turn and report cleanup confirmation.
    pub async fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentResult<AgentInterruptOutcome> {
        self.validate_agent_id(&params.agent_id)?;
        let identity = params.identity;
        let operation = self.inner.active.get(&identity.session_id).ok_or_else(|| {
            AgentError::run_not_found(format!(
                "active Agent turn `{}` was not found",
                identity.turn_id
            ))
        })?;
        let (active_identity, control, cleanup_pending) = match operation.value() {
            ActiveOperation::Reserved { identity, control } => {
                (identity.clone(), control.clone(), None)
            }
            ActiveOperation::Running(invocation) => (
                invocation.identity.clone(),
                invocation.control.clone(),
                None,
            ),
            ActiveOperation::CleanupPending {
                identity,
                control,
                task_count,
            } => (identity.clone(), control.clone(), Some(*task_count)),
            ActiveOperation::Maintenance { .. }
            | ActiveOperation::MaintenanceCleanupPending { .. } => {
                return Err(AgentError::run_not_found(
                    "Agent interrupt does not target a live Turn",
                ));
            }
        };
        drop(operation);
        if active_identity != identity {
            return Err(AgentError::run_not_found(format!(
                "active Agent turn `{}` was not found",
                identity.turn_id
            )));
        }
        if let Some(task_count) = cleanup_pending {
            return Ok(AgentInterruptOutcome::CleanupUnconfirmed { task_count });
        }
        let settlement = control
            .settle(RunTermination::Cancelled {
                policy: RUN_CANCELLATION_POLICY,
            })
            .await;
        Ok(interrupt_outcome(&settlement))
    }

    async fn drive_invocation(
        self,
        params: AgentRunParams,
        control: Arc<ActiveRunControl>,
        pipeline: AgentEventPipeline,
        provider_sink: AgentEventSink,
        start_rx: oneshot::Receiver<()>,
        settlement_tx: oneshot::Sender<AgentResult<AgentRunSettlement>>,
    ) {
        let identity = params.context.identity();
        tokio::select! {
            biased;
            () = control.cancellation().requested() => {
                let result = settle_failure(&pipeline, AgentError::new(
                    AgentErrorKind::Interrupted,
                    "Agent run was cancelled before Provider start",
                )).await;
                let _ = settlement_tx.send(result);
                self.remove_exact(&identity);
                return;
            }
            start = start_rx => {
                if start.is_err() {
                    let result = settle_failure(&pipeline, AgentError::run_not_found(
                        "Agent run start permit was dropped before execution",
                    )).await;
                    let _ = settlement_tx.send(result);
                    self.remove_exact(&identity);
                    return;
                }
            }
        }

        let (input_sender, input_receiver) = SessionTurnInputReceiver::channel();
        let (input_failure, mut input_failure_rx) = watch::channel(None);
        let invocation = ActiveInvocation {
            identity: identity.clone(),
            control: control.clone(),
            input: input_sender,
            pipeline: pipeline.clone(),
            input_failure,
        };
        if let Err(error) = self.activate(invocation) {
            let result = settle_failure(&pipeline, error).await;
            let _ = settlement_tx.send(result);
            self.remove_exact(&identity);
            return;
        }

        let session_context = params.context.session.clone();
        let session = match self.session(session_context, &control).await {
            Ok(session) => session.session,
            Err(error) => {
                let result = if matches!(
                    error.kind,
                    AgentErrorKind::RunFailure(talos_error::ErrorCode::RunCleanupUnconfirmed)
                ) {
                    Err(error)
                } else {
                    settle_failure(&pipeline, error).await
                };
                let _ = settlement_tx.send(result);
                self.remove_exact(&identity);
                return;
            }
        };
        let turn = SessionTurnParams {
            context: params.context,
            initial_input: params.input,
            continuation: params.continuation,
            input: input_receiver,
            events: provider_sink,
            control: AgentOperationControl::from_run_control(&control),
        };
        let provider_task = match control.tasks().spawn(move |_cancellation| async move {
            RunTaskCompletion::Confirmed(session.turn(turn).await)
        }) {
            Ok(task) => task,
            Err(error) => {
                let result = settle_failure(&pipeline, spawn_error(error)).await;
                let _ = settlement_tx.send(result);
                self.remove_exact(&identity);
                return;
            }
        };
        tokio::pin!(provider_task);
        let mut failure_before_cancellation = None;
        let (provider_result, lifecycle) = tokio::select! {
            biased;
            changed = input_failure_rx.changed() => {
                let input_error = changed
                    .ok()
                    .and_then(|()| input_failure_rx.borrow().clone());
                if input_error.is_some() {
                    if !control.cancellation().is_requested() {
                        failure_before_cancellation = input_error;
                    }
                    let lifecycle = control
                        .settle(RunTermination::Cancelled {
                            policy: RUN_CANCELLATION_POLICY,
                        })
                        .await;
                    (provider_task.await, lifecycle)
                } else {
                    let provider_result = provider_task.await;
                    let lifecycle = control
                        .settle(RunTermination::Completed {
                            timeout: RUN_COMPLETION_TIMEOUT,
                        })
                        .await;
                    (provider_result, lifecycle)
                }
            }
            provider_result = &mut provider_task => {
                let lifecycle = control
                    .settle(RunTermination::Completed {
                        timeout: RUN_COMPLETION_TIMEOUT,
                    })
                    .await;
                (provider_result, lifecycle)
            }
        };
        pipeline.wait_until_unreserved().await;
        let input_error = input_failure_rx.borrow().clone();
        if !cleanup_confirmed(&lifecycle) {
            self.mark_cleanup_pending(&identity, control, &lifecycle);
            let _ = settlement_tx.send(Err(cleanup_error(&lifecycle)));
            return;
        }

        let result = if let Some(error) = failure_before_cancellation {
            settle_failure(&pipeline, error).await
        } else if matches!(
            lifecycle.kind,
            SettlementKind::Cancelled(CancellationMode::Cooperative)
                | SettlementKind::Cancelled(CancellationMode::Forced)
        ) {
            settle_failure(
                &pipeline,
                AgentError::new(AgentErrorKind::Interrupted, "Agent run was interrupted"),
            )
            .await
        } else if let Some(error) = input_error {
            settle_failure(&pipeline, error).await
        } else {
            match provider_result {
                Ok(Ok(completion)) => match self.validate_turn_result_capabilities(&completion) {
                    Ok(()) => settle_provider_completion(&pipeline, completion).await,
                    Err(error) => settle_failure(&pipeline, error).await,
                },
                Ok(Err(error)) => settle_failure(&pipeline, error).await,
                Err(RunTaskError::Stopped) => {
                    settle_failure(
                        &pipeline,
                        AgentError::provider_error("Agent Provider task stopped"),
                    )
                    .await
                }
            }
        };
        let _ = settlement_tx.send(result);
        self.remove_exact(&identity);
    }
}
