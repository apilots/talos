use std::sync::atomic::Ordering;
use std::sync::Arc;

use dashmap::mapref::entry::Entry;
use talos_error::TalosError;
use talos_run_control::{
    ActiveRunControl, CancellationPolicy, RunControlScope, RunTaskCompletion, RunTermination,
};
use uuid::Uuid;

use super::settlement::{cleanup_confirmed, cleanup_error, spawn_error};
use super::{
    ActiveOperation, CachedSession, Harness, SessionOperationKind, SessionOperationOutcome,
    RUN_CANCELLATION_POLICY, RUN_COMPLETION_TIMEOUT,
};
use crate::{AgentOperationControl, AgentProvider, AgentResult, AgentSession, AgentSessionContext};

impl<P> Harness<P>
where
    P: AgentProvider,
{
    pub(super) async fn session(
        &self,
        context: AgentSessionContext,
        operation_control: &ActiveRunControl,
    ) -> AgentResult<CachedSession<P::Session>> {
        if let Some(cached) = self.inner.sessions.get(&context.session_id) {
            if cached.context != context {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Agent Session Context changed for a live Harness handle",
                ));
            }
            return Ok(cached.clone());
        }

        let session_id = context.session_id.clone();
        let control = Arc::new(ActiveRunControl::new(RunControlScope::SessionOperation {
            session_id: session_id.clone(),
            operation_id: format!("session_handle_{}", Uuid::new_v4().simple()),
        }));
        let acquisition = self.inner.provider.session(
            context.clone(),
            AgentOperationControl::from_run_control(&control),
        );
        let session = match tokio::select! {
            biased;
            () = operation_control.cancellation().requested() => {
                Err(TalosError::new(
                    talos_error::ErrorCode::Cancelled,
                    "Agent Session acquisition was cancelled",
                ))
            }
            result = acquisition => result
        } {
            Ok(session) => Arc::new(session),
            Err(error) => {
                let settlement = control
                    .settle(RunTermination::Cancelled {
                        policy: RUN_CANCELLATION_POLICY,
                    })
                    .await;
                if !cleanup_confirmed(&settlement) {
                    self.retain_session_cleanup(&session_id, control, &settlement);
                    return Err(cleanup_error(&settlement));
                }
                return Err(error);
            }
        };
        if !self.inner.accepting.load(Ordering::Acquire)
            || operation_control.cancellation().is_requested()
        {
            self.release_session_handle(
                &session_id,
                session,
                control,
                tokio::time::Instant::now() + RUN_COMPLETION_TIMEOUT,
            )
            .await?;
            return Err(TalosError::new(
                talos_error::ErrorCode::Cancelled,
                "Agent Session acquisition completed after shutdown began",
            ));
        }
        match self.inner.sessions.entry(session_id.clone()) {
            Entry::Vacant(entry) => {
                let cached = CachedSession {
                    context,
                    session,
                    control,
                };
                entry.insert(cached.clone());
                if !self.inner.accepting.load(Ordering::Acquire) {
                    self.inner.sessions.remove_if(&session_id, |_, current| {
                        Arc::ptr_eq(&current.session, &cached.session)
                    });
                    self.release_session_handle(
                        &session_id,
                        cached.session,
                        cached.control,
                        tokio::time::Instant::now() + RUN_COMPLETION_TIMEOUT,
                    )
                    .await?;
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Cancelled,
                        "Agent Session acquisition completed after shutdown began",
                    ));
                }
                return Ok(cached);
            }
            Entry::Occupied(_) => {}
        }
        self.release_session_handle(
            &session_id,
            session,
            control,
            tokio::time::Instant::now() + RUN_COMPLETION_TIMEOUT,
        )
        .await?;
        Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "Agent Session handle was acquired concurrently",
        ))
    }

    pub(super) async fn release_session_handle(
        &self,
        session_id: &str,
        session: Arc<P::Session>,
        session_control: Arc<ActiveRunControl>,
        deadline: tokio::time::Instant,
    ) -> AgentResult<()> {
        let control = Arc::new(ActiveRunControl::new(RunControlScope::SessionOperation {
            session_id: session_id.to_string(),
            operation_id: format!("session_release_{}", Uuid::new_v4().simple()),
        }));
        let outcome = self
            .execute_session_operation(
                session,
                SessionOperationKind::Close,
                control.clone(),
                deadline,
            )
            .await;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let session_settlement = session_control
            .settle(RunTermination::Cancelled {
                policy: CancellationPolicy { grace: remaining },
            })
            .await;
        if !cleanup_confirmed(&outcome.settlement) {
            self.retain_session_cleanup(session_id, control, &outcome.settlement);
            return Err(cleanup_error(&outcome.settlement));
        }
        if !cleanup_confirmed(&session_settlement) {
            self.retain_session_cleanup(session_id, session_control, &session_settlement);
            return Err(cleanup_error(&session_settlement));
        }
        outcome.result
    }

    /// Retain the existing Session slot when acquisition or disposal cannot confirm cleanup.
    fn retain_session_cleanup(
        &self,
        session_id: &str,
        control: Arc<ActiveRunControl>,
        settlement: &talos_run_control::Settlement,
    ) {
        if let Entry::Occupied(mut entry) = self.inner.active.entry(session_id.to_string()) {
            let identity = match entry.get() {
                ActiveOperation::Reserved { identity, .. }
                | ActiveOperation::CleanupPending { identity, .. } => Some(identity.clone()),
                ActiveOperation::Running(invocation) => Some(invocation.identity.clone()),
                ActiveOperation::Maintenance { .. }
                | ActiveOperation::MaintenanceCleanupPending { .. } => None,
            };
            entry.insert(match identity {
                Some(identity) => ActiveOperation::CleanupPending {
                    identity,
                    control,
                    task_count: super::settlement::cleanup_task_count(settlement),
                },
                None => ActiveOperation::MaintenanceCleanupPending { control },
            });
            self.inner.active_changed.notify_waiters();
        }
    }

    pub(super) async fn execute_session_operation(
        &self,
        session: Arc<P::Session>,
        kind: SessionOperationKind,
        control: Arc<ActiveRunControl>,
        deadline: tokio::time::Instant,
    ) -> SessionOperationOutcome {
        let provider_control = AgentOperationControl::from_run_control(&control);
        let task = control.tasks().spawn(move |_cancellation| async move {
            let result = match kind {
                SessionOperationKind::Clear => session.clear(provider_control).await,
                SessionOperationKind::Close => session.close(provider_control).await,
            };
            RunTaskCompletion::Confirmed(result)
        });
        let (result, termination) = match task {
            Ok(task) => {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                match tokio::time::timeout(remaining, task).await {
                    Ok(result) => (
                        match result {
                            Ok(result) => result,
                            Err(error) => Err(TalosError::from_source(
                                talos_error::ErrorCode::InternalError,
                                "Agent Session operation task stopped",
                                error,
                            )),
                        },
                        RunTermination::Completed {
                            timeout: deadline
                                .saturating_duration_since(tokio::time::Instant::now()),
                        },
                    ),
                    Err(_) => (
                        Err(TalosError::new(
                            talos_error::ErrorCode::Timeout,
                            "Agent Session operation timed out",
                        )),
                        RunTermination::Cancelled {
                            policy: CancellationPolicy {
                                grace: deadline
                                    .saturating_duration_since(tokio::time::Instant::now()),
                            },
                        },
                    ),
                }
            }
            Err(error) => (
                Err(spawn_error(error)),
                RunTermination::Completed {
                    timeout: deadline.saturating_duration_since(tokio::time::Instant::now()),
                },
            ),
        };
        let settlement = control.settle(termination).await;
        SessionOperationOutcome { result, settlement }
    }
}
