use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{mpsc, Mutex, Notify};
use uuid::Uuid;

use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockDelta, AgentMessageBlockDeltaContent, AgentMessageBlockStarted,
    AgentMessageBlockStatus, AgentMessageRole, AgentMessageStarted, AgentMessageStatus,
    AgentRunMetrics, AgentTurnRef,
};
use talos_error::TalosError;

use crate::{
    AgentEventReducer, AgentMessageBlockHandle, AgentMessageHandle, AgentResult, AgentRunCheckpoint,
};

const AGENT_PROGRESS_EVENT_CAPACITY: usize = 256;

struct AgentEventPipelineState {
    reducer: AgentEventReducer,
    event_tx: mpsc::Sender<AgentEvent>,
    reservation: Option<AgentEventReservationState>,
}

struct AgentEventReservationState {
    reservation_id: String,
    previous_reducer: AgentEventReducer,
}

struct AgentEventPipelineInner {
    sink_id: String,
    resume_checkpoint: AgentRunCheckpoint,
    state: Mutex<AgentEventPipelineState>,
    reservation_changed: Notify,
}

/// Harness-owned lifecycle authority backing one Provider event sink.
///
/// Harness records admission and settlement events through this owner. The
/// Provider receives only `AgentEventSink`, which cannot create User or Turn
/// lifecycle events.
#[derive(Clone)]
pub struct AgentEventPipeline {
    inner: Arc<AgentEventPipelineInner>,
}

impl AgentEventPipeline {
    /// Create a fresh Turn pipeline and its restricted Provider sink.
    pub fn fresh(identity: AgentTurnRef) -> (Self, AgentEventSink, mpsc::Receiver<AgentEvent>) {
        Self::build(
            AgentEventReducer::new(identity),
            AgentRunCheckpoint {
                last_sequence: 0,
                messages: Vec::new(),
                metrics: AgentRunMetrics::default(),
            },
        )
    }

    /// Restore a continuation pipeline from its canonical checkpoint.
    pub fn resumed(
        identity: AgentTurnRef,
        checkpoint: AgentRunCheckpoint,
    ) -> AgentResult<(Self, AgentEventSink, mpsc::Receiver<AgentEvent>)> {
        let reducer = AgentEventReducer::from_checkpoint(identity, checkpoint.clone())?;
        Ok(Self::build(reducer, checkpoint))
    }

    /// Record one Harness-owned admission or settlement event without placing
    /// it on the progress channel.
    pub async fn record(&self, payload: AgentEventPayload) -> AgentResult<AgentEvent> {
        record_event(&self.inner, payload, EventDelivery::ReturnOnly).await
    }

    /// Snapshot canonical reducer state after all recorded events.
    pub async fn checkpoint(&self) -> AgentRunCheckpoint {
        self.inner.state.lock().await.reducer.checkpoint()
    }

    /// Close open message and block state after abnormal Provider cleanup.
    ///
    /// Closure snapshots are delivered as progress so Service persists every
    /// lower sequence before committing the terminal settlement.
    pub(crate) async fn close_open_lifecycle(
        &self,
        message_status: AgentMessageStatus,
        block_status: AgentMessageBlockStatus,
    ) -> AgentResult<Vec<AgentEvent>> {
        loop {
            let notified = self.inner.reservation_changed.notified();
            let mut state = self.inner.state.lock().await;
            if state.reservation.is_some() {
                drop(state);
                notified.await;
                continue;
            }
            let mut next_reducer = state.reducer.clone();
            let events = next_reducer.close_open_lifecycle(message_status, block_status, now())?;
            for event in &events {
                state.event_tx.send(event.clone()).await.map_err(|_| {
                    TalosError::new(
                        talos_error::ErrorCode::InternalError,
                        "Agent progress event receiver was closed",
                    )
                })?;
            }
            state.reducer = next_reducer;
            return Ok(events);
        }
    }

    /// Record one completed User message before the Provider can start.
    pub async fn record_user_message(&self, message: AgentMessage) -> AgentResult<Vec<AgentEvent>> {
        let mut state = self.inner.state.lock().await;
        if state.reservation.is_some() {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Agent event pipeline already has a pending admission",
            ));
        }
        crate::event_admission::apply_user_message(&mut state.reducer, message)
    }

    /// Reserve one User message while Service persists its admission facts.
    ///
    /// Provider progress waits behind this reservation. Committing preserves
    /// the staged reducer state; dropping or aborting restores it.
    pub(crate) async fn reserve_user_message(
        &self,
        message: AgentMessage,
    ) -> AgentResult<(Vec<AgentEvent>, AgentEventReservation)> {
        let mut state = self.inner.state.lock().await;
        if state.reservation.is_some() {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Agent event pipeline already has a pending admission",
            ));
        }
        let previous_reducer = state.reducer.clone();
        let events = crate::event_admission::apply_user_message(&mut state.reducer, message)?;
        let reservation_id = format!("reservation_{}", Uuid::new_v4().simple());
        state.reservation = Some(AgentEventReservationState {
            reservation_id: reservation_id.clone(),
            previous_reducer,
        });
        Ok((
            events,
            AgentEventReservation {
                inner: self.inner.clone(),
                reservation_id: Some(reservation_id),
            },
        ))
    }

    /// Wait until the current admission reservation commits or aborts.
    pub(crate) async fn wait_until_unreserved(&self) {
        loop {
            let notified = self.inner.reservation_changed.notified();
            if self.inner.state.lock().await.reservation.is_none() {
                return;
            }
            notified.await;
        }
    }

    fn build(
        reducer: AgentEventReducer,
        resume_checkpoint: AgentRunCheckpoint,
    ) -> (Self, AgentEventSink, mpsc::Receiver<AgentEvent>) {
        let (event_tx, event_rx) = mpsc::channel(AGENT_PROGRESS_EVENT_CAPACITY);
        let inner = Arc::new(AgentEventPipelineInner {
            sink_id: format!("sink_{}", Uuid::new_v4().simple()),
            resume_checkpoint,
            state: Mutex::new(AgentEventPipelineState {
                reducer,
                event_tx,
                reservation: None,
            }),
            reservation_changed: Notify::new(),
        });
        (
            Self {
                inner: inner.clone(),
            },
            AgentEventSink { inner },
            event_rx,
        )
    }
}

/// Opaque reducer reservation held by one Harness input admission.
pub(crate) struct AgentEventReservation {
    inner: Arc<AgentEventPipelineInner>,
    reservation_id: Option<String>,
}

impl AgentEventReservation {
    /// Commit the prepared admission facts and unblock Provider progress.
    pub(crate) async fn commit(mut self) -> AgentResult<()> {
        let reservation_id = self
            .reservation_id
            .take()
            .expect("Agent event reservation is consumed by value");
        finish_reservation(&self.inner, &reservation_id, ReservationDisposition::Commit).await
    }
}

impl std::fmt::Debug for AgentEventReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AgentEventReservation")
    }
}

impl Drop for AgentEventReservation {
    fn drop(&mut self) {
        let Some(reservation_id) = self.reservation_id.take() else {
            return;
        };
        let inner = self.inner.clone();
        let task = tokio::spawn(async move {
            let _ =
                finish_reservation(&inner, &reservation_id, ReservationDisposition::Abort).await;
        });
        drop(task);
    }
}

/// Provider-restricted typed capability for canonical message and metrics events.
#[derive(Clone)]
pub struct AgentEventSink {
    inner: Arc<AgentEventPipelineInner>,
}

impl AgentEventSink {
    /// Start an Agent, Tool, or System message and return its scoped handle.
    pub async fn start_message(&self, role: AgentMessageRole) -> AgentResult<AgentMessageHandle> {
        if role == AgentMessageRole::User {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Provider event sink cannot create User messages",
            ));
        }
        let message_id = format!("message_{}", Uuid::new_v4().simple());
        let message = AgentMessage {
            message_id: message_id.clone(),
            turn_id: self.identity().await.turn_id,
            role,
            status: AgentMessageStatus::InProgress,
            blocks: Vec::new(),
            created_at: now(),
            completed_at: None,
        };
        self.emit(AgentEventPayload::MessageStarted(AgentMessageStarted {
            message,
        }))
        .await?;
        Ok(AgentMessageHandle {
            sink_id: self.inner.sink_id.clone(),
            message_id,
        })
    }

    /// Restore a message handle only when the checkpoint keeps it open.
    pub fn resume_message(&self, message_id: &str) -> AgentResult<AgentMessageHandle> {
        let message = self
            .inner
            .resume_checkpoint
            .messages
            .iter()
            .find(|message| message.message_id == message_id)
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "resumed Agent message does not exist",
                )
            })?;
        if message.role == AgentMessageRole::User
            || message.status != AgentMessageStatus::InProgress
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "resumed Agent message is not Provider-owned and open",
            ));
        }
        Ok(AgentMessageHandle {
            sink_id: self.inner.sink_id.clone(),
            message_id: message_id.to_string(),
        })
    }

    /// Start one typed block inside an open Provider-owned message.
    pub async fn start_block(
        &self,
        message: &AgentMessageHandle,
        content: AgentMessageBlockContent,
    ) -> AgentResult<AgentMessageBlockHandle> {
        self.validate_message_handle(message)?;
        let block_id = format!("block_{}", Uuid::new_v4().simple());
        let block = AgentMessageBlock {
            block_id: block_id.clone(),
            status: AgentMessageBlockStatus::Running,
            content,
            started_at: now(),
            completed_at: None,
        };
        self.emit(AgentEventPayload::MessageBlockStarted(
            AgentMessageBlockStarted {
                message_id: message.message_id.clone(),
                block,
            },
        ))
        .await?;
        Ok(AgentMessageBlockHandle {
            sink_id: self.inner.sink_id.clone(),
            message_id: message.message_id.clone(),
            block_id,
        })
    }

    /// Restore a block handle only when the checkpoint keeps it open.
    pub fn resume_block(
        &self,
        message_id: &str,
        block_id: &str,
    ) -> AgentResult<AgentMessageBlockHandle> {
        let message = self
            .inner
            .resume_checkpoint
            .messages
            .iter()
            .find(|message| message.message_id == message_id)
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "resumed Agent message does not exist",
                )
            })?;
        let block = message
            .blocks
            .iter()
            .find(|block| block.block_id == block_id)
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "resumed Agent block does not exist",
                )
            })?;
        if !matches!(
            block.status,
            AgentMessageBlockStatus::Running | AgentMessageBlockStatus::WaitingForInput
        ) {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "resumed Agent block is not open",
            ));
        }
        Ok(AgentMessageBlockHandle {
            sink_id: self.inner.sink_id.clone(),
            message_id: message_id.to_string(),
            block_id: block_id.to_string(),
        })
    }

    /// Append one kind-checked delta to an open block.
    pub async fn append_block_delta(
        &self,
        block: &AgentMessageBlockHandle,
        delta: AgentMessageBlockDeltaContent,
    ) -> AgentResult<()> {
        self.validate_block_handle(block)?;
        self.emit(AgentEventPayload::MessageBlockDelta(
            AgentMessageBlockDelta {
                message_id: block.message_id.clone(),
                block_id: block.block_id.clone(),
                delta,
            },
        ))
        .await?;
        Ok(())
    }

    /// Complete one block with a terminal lifecycle status.
    pub async fn complete_block(
        &self,
        block: &AgentMessageBlockHandle,
        status: AgentMessageBlockStatus,
    ) -> AgentResult<()> {
        self.validate_block_handle(block)?;
        let completed_at = now();
        let snapshot = {
            let state = self.inner.state.lock().await;
            state.reducer.completed_block_snapshot(
                &block.message_id,
                &block.block_id,
                status,
                completed_at,
            )?
        };
        self.emit(AgentEventPayload::MessageBlockCompleted(
            talos_agent_api::AgentMessageBlockCompleted {
                message_id: block.message_id.clone(),
                block: snapshot,
            },
        ))
        .await?;
        Ok(())
    }

    /// Complete one message with a terminal lifecycle status.
    pub async fn complete_message(
        &self,
        message: &AgentMessageHandle,
        status: AgentMessageStatus,
    ) -> AgentResult<()> {
        self.validate_message_handle(message)?;
        let snapshot = {
            let state = self.inner.state.lock().await;
            state
                .reducer
                .completed_message_snapshot(&message.message_id, status, now())?
        };
        self.emit(AgentEventPayload::MessageCompleted(snapshot))
            .await?;
        Ok(())
    }

    /// Publish a cumulative metrics snapshot.
    pub async fn update_metrics(&self, metrics: AgentRunMetrics) -> AgentResult<()> {
        self.emit(AgentEventPayload::MetricsUpdated(metrics))
            .await?;
        Ok(())
    }

    async fn emit(&self, payload: AgentEventPayload) -> AgentResult<AgentEvent> {
        record_event(&self.inner, payload, EventDelivery::Progress).await
    }

    async fn identity(&self) -> AgentTurnRef {
        self.inner.state.lock().await.reducer.identity().clone()
    }

    fn validate_message_handle(&self, handle: &AgentMessageHandle) -> AgentResult<()> {
        if handle.sink_id == self.inner.sink_id {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "Agent message handle belongs to another event sink",
        ))
    }

    fn validate_block_handle(&self, handle: &AgentMessageBlockHandle) -> AgentResult<()> {
        if handle.sink_id == self.inner.sink_id {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "Agent block handle belongs to another event sink",
        ))
    }
}

#[derive(Clone, Copy)]
enum EventDelivery {
    Progress,
    ReturnOnly,
}

#[derive(Clone, Copy)]
enum ReservationDisposition {
    Commit,
    Abort,
}

async fn record_event(
    inner: &Arc<AgentEventPipelineInner>,
    payload: AgentEventPayload,
    delivery: EventDelivery,
) -> AgentResult<AgentEvent> {
    loop {
        let notified = inner.reservation_changed.notified();
        let mut state = inner.state.lock().await;
        if state.reservation.is_some() {
            drop(state);
            notified.await;
            continue;
        }
        let event = AgentEvent {
            identity: state.reducer.identity().clone(),
            sequence: state.reducer.next_sequence(),
            occurred_at: now(),
            payload,
        };
        let mut next_reducer = state.reducer.clone();
        next_reducer.apply(event.clone())?;
        if matches!(delivery, EventDelivery::Progress) {
            state.event_tx.send(event.clone()).await.map_err(|_| {
                TalosError::new(
                    talos_error::ErrorCode::InternalError,
                    "Agent progress event receiver was closed",
                )
            })?;
        }
        state.reducer = next_reducer;
        return Ok(event);
    }
}

async fn finish_reservation(
    inner: &Arc<AgentEventPipelineInner>,
    reservation_id: &str,
    disposition: ReservationDisposition,
) -> AgentResult<()> {
    let mut state = inner.state.lock().await;
    let reservation = state.reservation.take().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::Conflict,
            "Agent event reservation is no longer active",
        )
    })?;
    if reservation.reservation_id != reservation_id {
        state.reservation = Some(reservation);
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "Agent event reservation identity does not match",
        ));
    }
    if matches!(disposition, ReservationDisposition::Abort) {
        state.reducer = reservation.previous_reducer;
    }
    drop(state);
    inner.reservation_changed.notify_waiters();
    Ok(())
}

fn now() -> i64 {
    Utc::now().timestamp()
}

#[cfg(test)]
#[path = "event_sink_tests.rs"]
mod tests;
