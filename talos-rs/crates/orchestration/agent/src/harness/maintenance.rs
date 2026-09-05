use std::sync::Arc;
use std::time::Duration;

use dashmap::mapref::entry::Entry;
use talos_agent_api::{AgentEventPayload, AgentStopReason, AgentTurnRef, AgentTurnStarted};
use talos_run_control::{
    ActiveRunControl, CancellationPolicy, RunControlScope, RunTaskCompletion, RunTermination,
};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::settlement::{
    cleanup_confirmed, cleanup_error, settle_completed, settle_failure, spawn_error,
};
use super::{ActiveOperation, Harness, SessionOperationKind, RUN_COMPLETION_TIMEOUT};
use crate::{
    event_sink::AgentEventPipeline, AgentAdmission, AgentAdmissionPermit, AgentContext, AgentError,
    AgentErrorKind, AgentEventSink, AgentOperationControl, AgentProgressEventReceiver,
    AgentProvider, AgentResult, AgentRunExecution, AgentRunSettlement, AgentRunSettlementReceiver,
    AgentSession, AgentSessionContext, SessionCompactionParams,
};

struct CompactionDriver {
    context: AgentContext,
    identity: AgentTurnRef,
    control: Arc<ActiveRunControl>,
    pipeline: AgentEventPipeline,
    sink: AgentEventSink,
    start_rx: oneshot::Receiver<()>,
    settlement_tx: oneshot::Sender<AgentResult<AgentRunSettlement>>,
}

impl<P> Harness<P>
where
    P: AgentProvider,
{
    /// Admit one user-requested compaction as a canonical Agent execution.
    pub async fn compact_session(&self, context: AgentContext) -> AgentResult<AgentRunExecution> {
        self.validate_agent_id(&context.session.agent_id)?;
        if !self.inner.capabilities.context_compaction {
            return Err(AgentError::capability_unsupported(
                &self.inner.agent_id,
                "contextCompaction",
            ));
        }
        let identity = context.identity();
        let control = Arc::new(ActiveRunControl::new(RunControlScope::SessionOperation {
            session_id: identity.session_id.clone(),
            operation_id: format!("compaction_{}", Uuid::new_v4().simple()),
        }));
        self.reserve(&identity, control.clone())?;
        let (pipeline, sink, progress) = AgentEventPipeline::fresh(identity.clone());
        let started = match pipeline
            .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
            .await
        {
            Ok(event) => event,
            Err(error) => {
                self.remove_exact(&identity);
                return Err(error);
            }
        };
        let (start_tx, start_rx) = oneshot::channel();
        let permit = AgentAdmissionPermit::new(async move {
            start_tx.send(()).map_err(|_| {
                AgentError::run_not_found("admitted Agent compaction is no longer active")
            })
        });
        let (settlement_tx, settlement_rx) = oneshot::channel();
        let harness = self.clone();
        let task = tokio::spawn(async move {
            harness
                .drive_compaction(CompactionDriver {
                    context,
                    identity,
                    control,
                    pipeline,
                    sink,
                    start_rx,
                    settlement_tx,
                })
                .await;
        });
        drop(task);
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: vec![started],
                permit,
            },
            events: AgentProgressEventReceiver::new(progress),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        })
    }

    /// Delete reconstructable Provider-owned state for one idle Session.
    pub async fn clear_session(&self, params: AgentSessionContext) -> AgentResult<()> {
        self.run_session_operation(params, SessionOperationKind::Clear)
            .await
    }

    /// Release live Provider resources for one idle Session.
    pub async fn close_session(&self, params: AgentSessionContext) -> AgentResult<()> {
        self.run_session_operation(params, SessionOperationKind::Close)
            .await
    }

    /// Stop admission, drain active operations, and release Provider resources.
    pub async fn shutdown(&self, timeout: Duration) -> AgentResult<()> {
        self.inner
            .accepting
            .store(false, std::sync::atomic::Ordering::Release);
        let deadline = tokio::time::Instant::now() + timeout;
        let active = self
            .inner
            .active
            .iter()
            .map(|operation| operation_control(operation.value()))
            .collect::<Vec<_>>();
        for control in active {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(AgentError::new(
                    AgentErrorKind::Timeout,
                    "Agent Harness shutdown timed out",
                ));
            }
            let settlement = tokio::time::timeout(
                remaining,
                control.settle(RunTermination::Cancelled {
                    policy: CancellationPolicy { grace: remaining },
                }),
            )
            .await
            .map_err(|_| {
                AgentError::new(AgentErrorKind::Timeout, "Agent Harness shutdown timed out")
            })?;
            if !cleanup_confirmed(&settlement) {
                return Err(cleanup_error(&settlement));
            }
        }

        loop {
            let active_changed = self.inner.active_changed.notified();
            if self.inner.active.is_empty() {
                break;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero()
                || tokio::time::timeout(remaining, active_changed)
                    .await
                    .is_err()
            {
                return Err(AgentError::new(
                    AgentErrorKind::Timeout,
                    "Agent Harness shutdown timed out waiting for active operations",
                ));
            }
        }

        let cached_sessions = self
            .inner
            .sessions
            .iter()
            .map(|cached| cached.clone())
            .collect::<Vec<_>>();
        for cached in cached_sessions {
            let session_id = cached.context.session_id.clone();
            let control = Arc::new(ActiveRunControl::new(RunControlScope::SessionOperation {
                session_id: session_id.clone(),
                operation_id: format!("shutdown_close_{}", Uuid::new_v4().simple()),
            }));
            let outcome = self
                .execute_session_operation(
                    cached.session.clone(),
                    SessionOperationKind::Close,
                    control,
                    deadline,
                )
                .await;
            if !cleanup_confirmed(&outcome.settlement) {
                return Err(cleanup_error(&outcome.settlement));
            }
            outcome.result?;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let session_settlement = cached
                .control
                .settle(RunTermination::Completed { timeout: remaining })
                .await;
            if !cleanup_confirmed(&session_settlement) {
                return Err(cleanup_error(&session_settlement));
            }
            self.inner.sessions.remove_if(&session_id, |_, current| {
                Arc::ptr_eq(&current.session, &cached.session)
            });
        }

        let operation_id = format!("shutdown_{}", Uuid::new_v4().simple());
        let control = Arc::new(ActiveRunControl::new(RunControlScope::ProcessOperation {
            operation_id,
        }));
        let provider = self.inner.provider.clone();
        let provider_control = AgentOperationControl::from_run_control(&control);
        let task = control
            .tasks()
            .spawn(move |_cancellation| async move {
                RunTaskCompletion::Confirmed(provider.shutdown(provider_control).await)
            })
            .map_err(spawn_error)?;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let result = match tokio::time::timeout(remaining, task).await {
            Ok(result) => result
                .map_err(|_| AgentError::provider_error("Agent Provider shutdown task stopped"))?,
            Err(_) => {
                let settlement = control
                    .settle(RunTermination::Cancelled {
                        policy: CancellationPolicy {
                            grace: deadline.saturating_duration_since(tokio::time::Instant::now()),
                        },
                    })
                    .await;
                if !cleanup_confirmed(&settlement) {
                    return Err(cleanup_error(&settlement));
                }
                return Err(AgentError::new(
                    AgentErrorKind::Timeout,
                    "Agent Harness shutdown timed out",
                ));
            }
        };
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let settlement = control
            .settle(RunTermination::Completed { timeout: remaining })
            .await;
        if !cleanup_confirmed(&settlement) {
            return Err(cleanup_error(&settlement));
        }
        result?;
        Ok(())
    }

    async fn drive_compaction(self, driver: CompactionDriver) {
        let CompactionDriver {
            context,
            identity,
            control,
            pipeline,
            sink,
            start_rx,
            settlement_tx,
        } = driver;
        tokio::select! {
            biased;
            () = control.cancellation().requested() => {
                let _ = settlement_tx.send(Err(AgentError::new(
                    AgentErrorKind::Interrupted,
                    "Agent compaction was cancelled before Provider start",
                )));
                self.remove_exact(&identity);
                return;
            }
            start = start_rx => {
                if start.is_err() {
                    let _ = settlement_tx.send(Err(AgentError::run_not_found(
                        "Agent compaction start permit was dropped",
                    )));
                    self.remove_exact(&identity);
                    return;
                }
            }
        }
        let session = match self.session(context.session.clone(), &control).await {
            Ok(session) => session.session,
            Err(error) => {
                let _ = settlement_tx.send(Err(error));
                self.remove_exact(&identity);
                return;
            }
        };
        let provider_sink = sink.clone();
        let params = SessionCompactionParams {
            context,
            events: sink,
            control: AgentOperationControl::from_run_control(&control),
        };
        let task = match control.tasks().spawn(move |_cancellation| async move {
            RunTaskCompletion::Confirmed(session.compact(params).await)
        }) {
            Ok(task) => task,
            Err(error) => {
                let _ = settlement_tx.send(Err(spawn_error(error)));
                self.remove_exact(&identity);
                return;
            }
        };
        let provider_result = task.await;
        let lifecycle = control
            .settle(RunTermination::Completed {
                timeout: RUN_COMPLETION_TIMEOUT,
            })
            .await;
        if !cleanup_confirmed(&lifecycle) {
            self.mark_cleanup_pending(&identity, control, &lifecycle);
            let _ = settlement_tx.send(Err(cleanup_error(&lifecycle)));
            return;
        }
        let result = match provider_result {
            Ok(Ok(compaction)) => {
                let mut metrics = pipeline.checkpoint().await.metrics;
                metrics.usage = compaction.usage;
                match provider_sink.update_metrics(metrics).await {
                    Ok(()) => settle_completed(&pipeline, AgentStopReason::EndTurn).await,
                    Err(error) => Err(error),
                }
            }
            Ok(Err(error)) => settle_failure(&pipeline, error).await,
            Err(_) => Err(AgentError::provider_error(
                "Agent Provider compaction task stopped",
            )),
        };
        let _ = settlement_tx.send(result);
        self.remove_exact(&identity);
    }

    async fn run_session_operation(
        &self,
        params: AgentSessionContext,
        kind: SessionOperationKind,
    ) -> AgentResult<()> {
        self.validate_agent_id(&params.agent_id)?;
        let session_id = params.session_id.clone();
        let (operation_id, control, previous_pending) =
            self.reserve_session_operation(&session_id, kind)?;
        let cached = match self.session(params, &control).await {
            Ok(session) => session,
            Err(error) => {
                self.restore_session_operation(&session_id, &operation_id, previous_pending);
                return Err(error);
            }
        };
        let outcome = self
            .execute_session_operation(
                cached.session.clone(),
                kind,
                control.clone(),
                tokio::time::Instant::now() + RUN_COMPLETION_TIMEOUT,
            )
            .await;
        if !cleanup_confirmed(&outcome.settlement) {
            self.keep_session_cleanup_pending(&session_id, &operation_id, control);
            return Err(cleanup_error(&outcome.settlement));
        }
        let result = outcome.result;
        if result.is_ok() && matches!(kind, SessionOperationKind::Close) {
            let session_settlement = cached
                .control
                .settle(RunTermination::Completed {
                    timeout: RUN_COMPLETION_TIMEOUT,
                })
                .await;
            if !cleanup_confirmed(&session_settlement) {
                self.keep_session_cleanup_pending(&session_id, &operation_id, cached.control);
                return Err(cleanup_error(&session_settlement));
            }
        }
        if result.is_ok() {
            if matches!(kind, SessionOperationKind::Close) {
                self.inner.sessions.remove_if(&session_id, |_, current| {
                    Arc::ptr_eq(&current.session, &cached.session)
                });
            }
            self.remove_session_operation(&session_id, &operation_id);
        } else {
            self.restore_session_operation(&session_id, &operation_id, previous_pending);
        }
        result
    }

    fn reserve_session_operation(
        &self,
        session_id: &str,
        kind: SessionOperationKind,
    ) -> AgentResult<(String, Arc<ActiveRunControl>, Option<ActiveOperation>)> {
        if !self
            .inner
            .accepting
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(AgentError::provider_error("Agent Harness is shutting down"));
        }
        let operation_id = format!("session_operation_{}", Uuid::new_v4().simple());
        let control = Arc::new(ActiveRunControl::new(RunControlScope::SessionOperation {
            session_id: session_id.to_string(),
            operation_id: operation_id.clone(),
        }));
        let reservation = match self.inner.active.entry(session_id.to_string()) {
            Entry::Vacant(entry) => {
                entry.insert(ActiveOperation::Maintenance {
                    operation_id: operation_id.clone(),
                    control: control.clone(),
                });
                (operation_id, control, None)
            }
            Entry::Occupied(mut entry)
                if matches!(kind, SessionOperationKind::Close)
                    && self.inner.sessions.contains_key(session_id)
                    && matches!(
                        entry.get(),
                        ActiveOperation::CleanupPending { .. }
                            | ActiveOperation::MaintenanceCleanupPending { .. }
                    ) =>
            {
                let previous = entry.get().clone();
                entry.insert(ActiveOperation::Maintenance {
                    operation_id: operation_id.clone(),
                    control: control.clone(),
                });
                (operation_id, control, Some(previous))
            }
            Entry::Occupied(_) => {
                return Err(AgentError::new(
                    AgentErrorKind::ActiveTurnRunning,
                    format!("session `{session_id}` already has an active Agent operation"),
                ));
            }
        };
        if !self
            .inner
            .accepting
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.restore_session_operation(session_id, &reservation.0, reservation.2.clone());
            return Err(AgentError::provider_error("Agent Harness is shutting down"));
        }
        Ok(reservation)
    }

    fn remove_session_operation(&self, session_id: &str, operation_id: &str) {
        if self.inner.active.get(session_id).is_some_and(|active| {
            matches!(
                active.value(),
                ActiveOperation::Maintenance {
                    operation_id: active_id,
                    ..
                } if active_id == operation_id
            )
        }) {
            self.inner.active.remove(session_id);
            self.inner.active_changed.notify_waiters();
        }
    }

    fn keep_session_cleanup_pending(
        &self,
        session_id: &str,
        operation_id: &str,
        control: Arc<ActiveRunControl>,
    ) {
        if let Entry::Occupied(mut entry) = self.inner.active.entry(session_id.to_string()) {
            if matches!(
                entry.get(),
                ActiveOperation::Maintenance {
                    operation_id: active_id,
                    ..
                } if active_id == operation_id
            ) {
                entry.insert(ActiveOperation::MaintenanceCleanupPending { control });
                self.inner.active_changed.notify_waiters();
            }
        }
    }

    fn restore_session_operation(
        &self,
        session_id: &str,
        operation_id: &str,
        previous_pending: Option<ActiveOperation>,
    ) {
        if let Entry::Occupied(mut entry) = self.inner.active.entry(session_id.to_string()) {
            if matches!(
                entry.get(),
                ActiveOperation::Maintenance {
                    operation_id: active_id,
                    ..
                } if active_id == operation_id
            ) {
                if let Some(previous_pending) = previous_pending {
                    entry.insert(previous_pending);
                } else {
                    entry.remove();
                }
                self.inner.active_changed.notify_waiters();
            }
        }
    }
}

fn operation_control(operation: &ActiveOperation) -> Arc<ActiveRunControl> {
    match operation {
        ActiveOperation::Reserved { control, .. }
        | ActiveOperation::CleanupPending { control, .. }
        | ActiveOperation::Maintenance { control, .. }
        | ActiveOperation::MaintenanceCleanupPending { control, .. } => control.clone(),
        ActiveOperation::Running(invocation) => invocation.control.clone(),
    }
}
