use super::AgentHarnessImpl;
use crate::{AgentResult, AgentRunParams, AgentRunResult};
use dashmap::mapref::entry::Entry;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::AgentEvent;
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

/// Harness-owned abort handle for a runtime execution.
///
/// Runtime implementations can expose backend abort mechanics through this
/// wrapper, but callers outside Harness must use AgentInterruptParams instead.
#[derive(Clone)]
pub(crate) struct AgentRuntimeAbortHandle {
    outer: Option<AbortHandle>,
    inner: Arc<Mutex<Option<AbortHandle>>>,
    cancelled: Arc<AtomicBool>,
}

impl AgentRuntimeAbortHandle {
    /// Create an abort handle for a newly spawned runtime task.
    pub(crate) fn new(outer: AbortHandle) -> Self {
        Self {
            outer: Some(outer),
            inner: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn inner_only() -> Self {
        Self {
            outer: None,
            inner: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn attach_inner(&self, handle: AbortHandle) {
        self.inner.lock().unwrap().replace(handle);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Abort all tracked runtime tasks for the active Harness run.
    pub(crate) fn abort(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(outer) = &self.outer {
            outer.abort();
        }
        if let Some(inner) = self.inner.lock().unwrap().as_ref() {
            inner.abort();
        }
    }
}

/// Harness-internal operation selected for a queued or active run.
///
/// The mode decides which AgentRuntime SPI method is called after lifecycle
/// reservation. It must not be exposed as a Service, Gateway, or SDK field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentRunMode {
    /// Start a new Agent run for a fresh turn.
    Start,
    /// Continue the original Agent run for the same turn_id.
    Resume,
}

/// Harness-owned in-memory state for an active Agent run.
///
/// This state is owned by AgentHarness lifecycle code only. It represents the
/// currently reserved or running Talos turn and must not be backed by Service
/// active-turn records or runtime-private task identifiers.
#[derive(Clone)]
pub(crate) enum ActiveTurnHandle {
    /// A session slot has been reserved before the runtime handle exists.
    Reserved,
    /// A runtime task is running for a concrete Talos turn.
    Running {
        /// Talos turn id; this is also the Agent run identity.
        turn_id: String,
        /// Abort handle owned by the concrete runtime task.
        execution_task: AgentRuntimeAbortHandle,
        /// Bounded input sender for the exact running turn.
        message_tx: Option<mpsc::Sender<String>>,
    },
}

/// Queued Agent run waiting for the session's active run to finish.
///
/// The queue is a Harness lifecycle concern. Runtime implementations should
/// never maintain their own Talos session backlog or see Service store records.
pub(crate) enum QueuedTurn {
    /// Non-streaming run waiting for a result channel.
    Unary {
        /// Runtime operation to execute after dequeue.
        mode: AgentRunMode,
        /// Shared Agent run parameters.
        request: AgentRunParams,
        /// Result channel for the queued caller.
        response_tx: oneshot::Sender<AgentResult<AgentRunResult>>,
    },
    /// Streaming run waiting for its event channel to be connected.
    Stream {
        /// Runtime operation to execute after dequeue.
        mode: AgentRunMode,
        /// Shared Agent run parameters.
        request: AgentRunParams,
        /// Event channel already returned to the queued caller.
        event_tx: mpsc::Sender<AgentEvent>,
    },
}

impl AgentHarnessImpl {
    /// Reserve the Harness lifecycle slot for a session before runtime start.
    ///
    /// A Reserved slot prevents concurrent turns from starting while the
    /// runtime stream or task is being constructed.
    pub(crate) fn try_reserve_active_turn(&self, session_id: &str) -> TalosResult<()> {
        match self.active_turns.entry(session_id.to_string()) {
            Entry::Occupied(_) => Err(TalosError::public(
                talos_error::ErrorCode::ActiveTurnRunning,
                format!("session `{session_id}` already has an active turn"),
            )),
            Entry::Vacant(entry) => {
                entry.insert(ActiveTurnHandle::Reserved);
                Ok(())
            }
        }
    }

    /// Attach a runtime abort handle to a previously reserved session slot.
    ///
    /// The provided turn id is the Talos turn id and the Agent run identity.
    pub(crate) fn activate_reserved_turn(
        &self,
        session_id: &str,
        turn_id: String,
        execution_task: AgentRuntimeAbortHandle,
        message_tx: Option<mpsc::Sender<String>>,
    ) -> TalosResult<()> {
        match self.active_turns.entry(session_id.to_string()) {
            Entry::Occupied(mut entry) => {
                entry.insert(ActiveTurnHandle::Running {
                    turn_id,
                    execution_task,
                    message_tx,
                });
                Ok(())
            }
            Entry::Vacant(_) => Err(TalosError::internal(format!(
                "missing reserved active turn slot for session `{session_id}`"
            ))),
        }
    }

    /// Remove the Harness active-run slot for a session.
    pub(crate) fn clear_active_turn(&self, session_id: &str) {
        self.active_turns.remove(session_id);
    }

    /// Enqueue a run behind the active run for the same session.
    pub(crate) async fn enqueue_turn(&self, session_id: &str, queued_turn: QueuedTurn) -> usize {
        let mut backlogs = self.session_backlogs.lock().await;
        let backlog = backlogs
            .entry(session_id.to_string())
            .or_insert_with(VecDeque::new);
        backlog.push_back(queued_turn);
        backlog.len()
    }

    /// Pop the next queued run for a session and clean up empty backlog state.
    pub(crate) async fn dequeue_next_turn(&self, session_id: &str) -> Option<QueuedTurn> {
        let mut backlogs = self.session_backlogs.lock().await;
        let backlog = backlogs.get_mut(session_id)?;
        let next = backlog.pop_front();
        if backlog.is_empty() {
            backlogs.remove(session_id);
        }
        next
    }

    /// Return true when Harness has queued runs for a session.
    pub(crate) async fn has_queued_turns(&self, session_id: &str) -> bool {
        self.session_backlogs
            .lock()
            .await
            .get(session_id)
            .is_some_and(|backlog| !backlog.is_empty())
    }

    /// Return true when Harness currently owns an active run slot.
    pub(crate) fn has_active_turn(&self, session_id: &str) -> bool {
        self.active_turns.contains_key(session_id)
    }

    /// Wait for Harness-owned active runs to finish or force-abort on timeout.
    ///
    /// This drain only touches in-memory runtime handles. It does not mutate
    /// Service active-turn records or timeline recovery state.
    pub(crate) async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.active_turns.is_empty() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                let remaining: Vec<_> = self
                    .active_turns
                    .iter()
                    .map(|entry| entry.key().clone())
                    .collect();
                for session_id in &remaining {
                    if let Some(handle) = self.active_turns.get(session_id) {
                        if let ActiveTurnHandle::Running { execution_task, .. } = handle.value() {
                            execution_task.abort();
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                for session_id in &remaining {
                    self.clear_active_turn(session_id);
                }
                return Err(TalosError::internal(format!(
                    "drain_active_turns timed out with {} sessions still active: {:?}",
                    remaining.len(),
                    remaining
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Reserve a session for a unary run or enqueue an internal resume.
    ///
    /// This method is the Harness lifecycle gate for non-streaming execution.
    /// It must not call concrete runtime code or mutate Service active-turn
    /// records; it only decides whether the Harness can start now.
    pub(crate) async fn reserve_or_enqueue_unary_turn(
        &self,
        mode: AgentRunMode,
        request: AgentRunParams,
    ) -> AgentResult<Option<oneshot::Receiver<AgentResult<AgentRunResult>>>> {
        let session_id = request.context.session_id.clone();
        if mode == AgentRunMode::Start {
            self.try_reserve_active_turn(&session_id)?;
            return Ok(None);
        }
        if self.has_queued_turns(&session_id).await {
            let (response_tx, response_rx) = oneshot::channel();
            self.enqueue_turn(
                &session_id,
                QueuedTurn::Unary {
                    mode,
                    request,
                    response_tx,
                },
            )
            .await;
            return Ok(Some(response_rx));
        }

        match self.try_reserve_active_turn(&session_id) {
            Ok(()) => Ok(None),
            Err(_) => {
                let (response_tx, response_rx) = oneshot::channel();
                self.enqueue_turn(
                    &session_id,
                    QueuedTurn::Unary {
                        mode,
                        request,
                        response_tx,
                    },
                )
                .await;
                Ok(Some(response_rx))
            }
        }
    }

    /// Reserve a session for a streaming run or enqueue an internal resume.
    ///
    /// This method owns Harness backlog ordering for streamed execution. The
    /// returned boolean means the caller may start the runtime immediately.
    pub(crate) async fn reserve_or_enqueue_stream_turn(
        &self,
        mode: AgentRunMode,
        request: AgentRunParams,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> AgentResult<bool> {
        let session_id = request.context.session_id.clone();
        if mode == AgentRunMode::Start {
            self.try_reserve_active_turn(&session_id)?;
            return Ok(true);
        }
        if self.has_queued_turns(&session_id).await {
            self.enqueue_turn(
                &session_id,
                QueuedTurn::Stream {
                    mode,
                    request,
                    event_tx,
                },
            )
            .await;
            return Ok(false);
        }

        match self.try_reserve_active_turn(&session_id) {
            Ok(()) => Ok(true),
            Err(_) => {
                self.enqueue_turn(
                    &session_id,
                    QueuedTurn::Stream {
                        mode,
                        request,
                        event_tx,
                    },
                )
                .await;
                Ok(false)
            }
        }
    }

    /// Clear the finished active turn and dispatch the next queued run.
    ///
    /// The next queued run receives a fresh Reserved slot before runtime start,
    /// keeping active-turn ownership inside the Harness lifecycle state.
    pub(crate) async fn finish_active_turn_and_dispatch_next(&self, session_id: &str) {
        if let Some(queued_turn) = self.dequeue_next_turn(session_id).await {
            self.active_turns
                .insert(session_id.to_string(), ActiveTurnHandle::Reserved);
            self.dispatch_queued_turn(queued_turn);
        } else {
            self.clear_active_turn(session_id);
        }
    }

    /// Dispatch a queued run after the Harness lifecycle slot is reserved.
    ///
    /// This method bridges lifecycle queue state to the existing run helpers;
    /// it must not introduce runtime-specific scheduling behavior.
    fn dispatch_queued_turn(&self, queued_turn: QueuedTurn) {
        match queued_turn {
            QueuedTurn::Unary {
                mode,
                request,
                response_tx,
            } => {
                let this = self.clone();
                let task = tokio::spawn(async move {
                    let result = this.execute_reserved_run(mode, request).await;
                    let _ = response_tx.send(result);
                });
                drop(task);
            }
            QueuedTurn::Stream {
                mode,
                request,
                event_tx,
            } => {
                self.spawn_reserved_stream_turn(mode, request, event_tx);
            }
        }
    }
}
