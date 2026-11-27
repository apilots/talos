//! Optional durable AgentHarness decorator assembled by a Worker host.
//! The host exclusively opens and owns its WorkerExecutionStore namespace; the
//! underlying Provider SPI and Harness core do not depend on product Session Stores.
//! Every ingress must share this decorator and its original Harness instance.

use crate::{AgentEventReducer, AgentHarness};
use agent_protocol::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use talos_agent_api::{AgentEvent, AgentEventPayload};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::execution::WorkerExecutionStore;
use tokio::sync::{mpsc, oneshot};

#[path = "local_execution_drive.rs"]
mod drive;

struct InputCommit {
    command: AgentExecutionCommand,
    admission: AgentAdmission,
    result: oneshot::Sender<TalosResult<()>>,
}
#[derive(Clone)]
struct Running {
    session: AgentSessionContext,
    inputs: mpsc::Sender<InputCommit>,
}
/// Bounded Worker-local completion policy invoked before terminal persistence.
pub type LocalTerminalObserver = dyn Fn(AgentContext, AgentEvent) -> AgentFuture<'static, Vec<AgentMemoryModelUsage>>
    + Send
    + Sync;

struct RecordedInvocation {
    session: AgentSessionContext,
    identity: talos_agent_api::AgentTurnRef,
    context: Option<AgentContext>,
    reducer: AgentEventReducer,
    events: AgentProgressEventReceiver,
    settlement: AgentRunSettlementReceiver,
    inputs: mpsc::Receiver<InputCommit>,
    delivery: mpsc::Sender<AgentEvent>,
}

#[derive(Clone)]
pub struct LocalExecutionHarness {
    inner: Arc<dyn AgentHarness>,
    store: Arc<WorkerExecutionStore>,
    active: Arc<Mutex<HashMap<(String, String), Running>>>,
    terminal_observer: Option<Arc<LocalTerminalObserver>>,
}

impl LocalExecutionHarness {
    pub fn new(inner: Arc<dyn AgentHarness>, store: Arc<WorkerExecutionStore>) -> Self {
        Self {
            inner,
            store,
            active: Default::default(),
            terminal_observer: None,
        }
    }

    /// Install a local bounded Memory collaborator before terminal persistence.
    /// The observer must not fetch product history or replace the main Turn result.
    pub fn with_terminal_observer(mut self, observer: Arc<LocalTerminalObserver>) -> Self {
        self.terminal_observer = Some(observer);
        self
    }

    fn record(
        &self,
        session: AgentSessionContext,
        command: AgentExecutionCommand,
        execution: AgentRunExecution,
        checkpoint: Option<AgentRunCheckpoint>,
    ) -> TalosResult<AgentRunExecution> {
        let identity = execution
            .admission
            .events
            .first()
            .ok_or_else(|| TalosError::internal("Worker admission has no execution facts."))?
            .identity
            .clone();
        let mut reducer = match checkpoint {
            Some(checkpoint) => AgentEventReducer::from_checkpoint(identity.clone(), checkpoint)?,
            None => AgentEventReducer::new(identity.clone()),
        };
        for event in &execution.admission.events {
            reducer.apply(event.clone())?;
        }
        let events = execution.admission.events.clone();
        let original_events = events.clone();
        let (tx, rx) = mpsc::channel(32);
        let (settled_tx, settled_rx) = oneshot::channel();
        let (input_tx, input_rx) = mpsc::channel(8);
        let recorder = self.clone();
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events,
                permit: AgentAdmissionPermit::new(async move {
                    recorder
                        .store
                        .admit(&session, &command, original_events, &reducer.checkpoint())
                        .await?;
                    recorder
                        .active
                        .lock()
                        .expect("Worker execution lock")
                        .insert(
                            (identity.session_id.clone(), identity.turn_id.clone()),
                            Running {
                                session: session.clone(),
                                inputs: input_tx,
                            },
                        );
                    let delivery =
                        tokio::spawn(async move { execution.admission.permit.consume().await })
                            .await
                            .map_err(|error| {
                                TalosError::internal_with_source(
                                    "Worker admission delivery task stopped.",
                                    error,
                                )
                            })
                            .and_then(|result| result);
                    if let Err(error) = delivery {
                        recorder
                            .active
                            .lock()
                            .expect("Worker execution lock")
                            .remove(&(identity.session_id.clone(), identity.turn_id.clone()));
                        let result = recorder
                            .fail_execution(&session, &identity, error.clone())
                            .await;
                        let _ = settled_tx.send(result);
                        return Ok(());
                    }
                    tokio::spawn(async move {
                        let context = match command {
                            AgentExecutionCommand::Run { params } => Some(params.context),
                            _ => None,
                        };
                        let result = recorder
                            .drive(RecordedInvocation {
                                session: session.clone(),
                                identity: identity.clone(),
                                context,
                                reducer,
                                events: execution.events,
                                settlement: execution.settlement,
                                inputs: input_rx,
                                delivery: tx,
                            })
                            .await;
                        recorder
                            .active
                            .lock()
                            .expect("Worker execution lock")
                            .remove(&(identity.session_id.clone(), identity.turn_id.clone()));
                        if let Err(Err(error)) = settled_tx.send(result) {
                            talos_logging::report_error(&error);
                        }
                    });
                    Ok(())
                }),
            },
            events: AgentProgressEventReceiver::new(rx),
            settlement: AgentRunSettlementReceiver::new(settled_rx),
        })
    }

    async fn replay(
        &self,
        session: AgentSessionContext,
        events: Vec<AgentEvent>,
    ) -> TalosResult<AgentRunExecution> {
        let identity = events
            .first()
            .ok_or_else(|| TalosError::internal("Worker replay admission is empty."))?
            .identity
            .clone();
        let sequence = events.last().expect("nonempty admission").sequence;
        let (tx, rx) = mpsc::channel(32);
        let (settled_tx, settled_rx) = oneshot::channel();
        let (start_tx, start_rx) = oneshot::channel();
        let store = self.store.clone();
        tokio::spawn(async move {
            if start_rx.await.is_err() {
                return;
            }
            let mut after_cursor = None;
            loop {
                let page = match store
                    .read_events(AgentSessionEventsParams {
                        session: session.clone(),
                        after_cursor,
                        limit: MAX_EXECUTION_PAGE_EVENTS,
                    })
                    .await
                {
                    Ok(page) => page,
                    Err(error) => {
                        let _ = settled_tx.send(Err(error));
                        return;
                    }
                };
                for committed in page.events {
                    if committed.event.identity != identity || committed.event.sequence <= sequence
                    {
                        continue;
                    }
                    if let Some(settlement) = committed.settlement {
                        let outcome = match settlement {
                            AgentExecutionSettlement::WaitingForInput {
                                continuation,
                                metrics,
                            } => AgentRunSettlement::WaitingForInput {
                                event: committed.event,
                                continuation,
                                metrics,
                            },
                            AgentExecutionSettlement::Finished { metrics, .. } => {
                                let result = match &committed.event.payload {
                                    AgentEventPayload::TurnFailed(failed) => {
                                        Err(failed.error.clone().into())
                                    }
                                    _ => Ok(()),
                                };
                                AgentRunSettlement::Finished {
                                    event: committed.event,
                                    metrics,
                                    result,
                                }
                            }
                        };
                        drop(tx);
                        let _ = settled_tx.send(Ok(outcome));
                        return;
                    }
                    if tx.send(committed.event).await.is_err() {
                        return;
                    }
                }
                after_cursor = Some(page.next_cursor);
                if !page.has_more {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        });
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events,
                permit: AgentAdmissionPermit::new(async move {
                    start_tx.send(()).map_err(|_| {
                        TalosError::new(
                            ErrorCode::ServiceUnavailable,
                            "Worker replay closed before admission.",
                        )
                    })
                }),
            },
            events: AgentProgressEventReceiver::new(rx),
            settlement: AgentRunSettlementReceiver::new(settled_rx),
        })
    }
}

impl AgentHarness for LocalExecutionHarness {
    fn read_workspace_memory(
        &self,
        context: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        self.inner.read_workspace_memory(context)
    }

    fn update_workspace_memory(
        &self,
        context: AgentSessionContext,
        update: talos_memory::UserWorkspaceMemoryUpdate,
    ) -> AgentFuture<'_, AgentResult<talos_memory::UserWorkspaceMemorySnapshot>> {
        self.inner.update_workspace_memory(context, update)
    }

    fn form_dream_memory(
        &self,
        context: AgentSessionContext,
        model_call: DreamMemoryModelCall,
    ) -> AgentFuture<'_, AgentResult<Option<u64>>> {
        self.inner.form_dream_memory(context, model_call)
    }

    fn read_session_execution_state(
        &self,
        session: AgentSessionContext,
    ) -> AgentFuture<'_, AgentResult<Option<SessionExecutionState>>> {
        Box::pin(async move {
            Ok(Some(self.store.execution_state(&session).await?.unwrap_or(
                SessionExecutionState {
                    history_cursor: Default::default(),
                    active_turn: None,
                    coordinator_id: None,
                    handoff_ready: true,
                },
            )))
        })
    }

    fn read_execution_payload(
        &self,
        params: AgentExecutionPayloadParams,
    ) -> AgentFuture<'_, AgentResult<AgentExecutionPayloadResult>> {
        Box::pin(async move { self.store.read_payload(params).await })
    }
    fn read_session_events(
        &self,
        params: AgentSessionEventsParams,
    ) -> AgentFuture<'_, AgentResult<AgentSessionEventsResult>> {
        Box::pin(async move { self.store.read_events(params).await })
    }
    fn start(&self, mut params: AgentRunParams) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(async move {
            let session = params.context.session.clone();
            if self.store.is_retired(&session.session_id).await? {
                return Err(TalosError::new(
                    ErrorCode::SessionNotFound,
                    "Worker Session was permanently deleted.",
                ));
            }
            let state = self.store.execution_state(&session).await?;
            let generation = state
                .as_ref()
                .map_or(0, |state| state.history_cursor.history_generation);
            if params
                .context
                .history_cursor
                .as_ref()
                .map_or(0, |cursor| cursor.history_generation)
                != generation
            {
                return Err(TalosError::new(
                    ErrorCode::EventCursorExpired,
                    "Worker command targets a cleared Session generation.",
                ));
            }
            let command = AgentExecutionCommand::Run {
                params: Box::new(params.clone()),
            };
            if let Some(events) = self
                .store
                .admission(&session, &params.context.identity(), &command)
                .await?
            {
                return self.replay(session, events).await;
            }
            params.context.history_cursor = self
                .store
                .execution_state(&session)
                .await?
                .map(|state| state.history_cursor)
                .or_else(|| Some(Default::default()));
            if params.continuation.is_some() {
                let identity = params.context.identity();
                let stored = self.store.settlement(&identity).await?.ok_or_else(|| {
                    TalosError::new(ErrorCode::Conflict, "Worker continuation is unavailable.")
                })?;
                match stored.1 {
                    AgentExecutionSettlement::WaitingForInput { continuation, .. }
                        if Some(&continuation) == params.continuation.as_ref() =>
                    {
                        params.checkpoint = self.store.checkpoint(&identity).await?
                    }
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Worker continuation no longer matches this Turn.",
                        ))
                    }
                }
            }
            let checkpoint = params.checkpoint.clone();
            let execution = self.inner.start(params).await?;
            self.record(session, command, execution, checkpoint)
        })
    }
    fn submit_input(
        &self,
        params: AgentInputParams,
    ) -> AgentFuture<'_, AgentResult<AgentAdmission>> {
        Box::pin(async move {
            let running = self
                .active
                .lock()
                .expect("Worker execution lock")
                .get(&(
                    params.identity.session_id.clone(),
                    params.identity.turn_id.clone(),
                ))
                .cloned()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::Conflict,
                        "Worker input targets no active execution.",
                    )
                })?;
            let command = AgentExecutionCommand::Input {
                params: Box::new(params.clone()),
            };
            if let Some(events) = self
                .store
                .admission(&running.session, &params.identity, &command)
                .await?
            {
                return Ok(AgentAdmission {
                    events,
                    permit: AgentAdmissionPermit::new(async { Ok(()) }),
                });
            }
            let admission = self.inner.submit_input(params).await?;
            let events = admission.events.clone();
            Ok(AgentAdmission {
                events,
                permit: AgentAdmissionPermit::new(async move {
                    let (tx, rx) = oneshot::channel();
                    running
                        .inputs
                        .send(InputCommit {
                            command,
                            admission,
                            result: tx,
                        })
                        .await
                        .map_err(|_| {
                            TalosError::new(
                                ErrorCode::Conflict,
                                "Worker input execution already settled.",
                            )
                        })?;
                    rx.await.map_err(|error| {
                        TalosError::internal_with_source("Worker input commit stopped.", error)
                    })?
                }),
            })
        })
    }
    fn compact_session(
        &self,
        context: AgentContext,
    ) -> AgentFuture<'_, AgentResult<AgentRunExecution>> {
        Box::pin(async move {
            let command = AgentExecutionCommand::Compaction {
                context: Box::new(context.clone()),
            };
            let session = context.session.clone();
            if let Some(events) = self
                .store
                .admission(&session, &context.identity(), &command)
                .await?
            {
                return self.replay(session, events).await;
            }
            let execution = self.inner.compact_session(context).await?;
            self.record(session, command, execution, None)
        })
    }
    fn interrupt(
        &self,
        params: AgentInterruptParams,
    ) -> AgentFuture<'_, AgentResult<talos_agent_api::AgentInterruptOutcome>> {
        Box::pin(async move {
            let scope = self
                .store
                .session_scope(&params.identity.session_id)
                .await?;
            let waiting =
                self.store
                    .settlement(&params.identity)
                    .await?
                    .is_some_and(|(_, settlement)| {
                        matches!(settlement, AgentExecutionSettlement::WaitingForInput { .. })
                    });
            if waiting
                && !self
                    .active
                    .lock()
                    .expect("Worker execution lock")
                    .contains_key(&(
                        params.identity.session_id.clone(),
                        params.identity.turn_id.clone(),
                    ))
            {
                let scope = scope.ok_or_else(|| {
                    TalosError::new(ErrorCode::SessionNotFound, "Worker Session is unavailable.")
                })?;
                if scope.agent_id != params.agent_id {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "Agent does not own this Worker Session.",
                    ));
                }
                // A persisted waiting settlement proves the Provider invocation has exited.
                // There is no live Provider effect to cancel; settle its continuation locally.
                self.fail_execution(
                    &scope,
                    &params.identity,
                    TalosError::new(ErrorCode::Cancelled, "Agent Turn was cancelled."),
                )
                .await?;
                return Ok(talos_agent_api::AgentInterruptOutcome::Cancelled);
            }
            let outcome = match self.inner.interrupt(params.clone()).await {
                Ok(outcome) => outcome,
                Err(error)
                    if error.is_code(ErrorCode::ResourceNotFound)
                        && scope.is_some()
                        && !self
                            .active
                            .lock()
                            .expect("Worker execution lock")
                            .contains_key(&(
                                params.identity.session_id.clone(),
                                params.identity.turn_id.clone(),
                            )) =>
                {
                    talos_agent_api::AgentInterruptOutcome::Cancelled
                }
                Err(error) => return Err(error),
            };
            if !matches!(
                outcome,
                talos_agent_api::AgentInterruptOutcome::CleanupUnconfirmed { .. }
            ) && !self
                .active
                .lock()
                .expect("Worker execution lock")
                .contains_key(&(
                    params.identity.session_id.clone(),
                    params.identity.turn_id.clone(),
                ))
            {
                if let Some(scope) = scope {
                    if self
                        .store
                        .execution_state(&scope)
                        .await?
                        .is_some_and(|state| state.active_turn.as_ref() == Some(&params.identity))
                    {
                        self.fail_execution(
                            &scope,
                            &params.identity,
                            TalosError::new(ErrorCode::Cancelled, "Agent Turn was cancelled."),
                        )
                        .await?;
                    }
                }
            }
            Ok(outcome)
        })
    }
    fn clear_session(
        &self,
        session: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async move {
            if let Some(state) = self.store.execution_state(&session).await? {
                if state.history_cursor.history_generation
                    == expected.history_generation.saturating_add(1)
                    && state.history_cursor.completed_turn_id.is_none()
                    && state.active_turn.is_none()
                {
                    return Ok(());
                }
                if state.history_cursor.history_generation != expected.history_generation {
                    return Err(TalosError::new(
                        ErrorCode::EventCursorExpired,
                        "Worker clear targets another Session generation.",
                    ));
                }
            }
            self.inner.clear_session(session.clone(), expected).await?;
            self.store.clear(&session).await
        })
    }
    fn retire_session(
        &self,
        session: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        Box::pin(async move {
            if self.store.is_retired(&session.session_id).await? {
                return Ok(());
            }
            self.inner
                .clear_session(session.clone(), expected.clone())
                .await?;
            self.inner.close_session(session.clone(), expected).await?;
            self.store.retire(&session).await
        })
    }
    fn close_session(
        &self,
        session: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, AgentResult<()>> {
        self.inner.close_session(session, expected)
    }
    fn shutdown(&self, timeout: Duration) -> AgentFuture<'_, AgentResult<()>> {
        self.inner.shutdown(timeout)
    }
}
