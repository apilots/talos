use std::sync::Arc;

use crate::TimelineEventService;
use talos_error::{TalosError, TalosResult};
use talos_store::session::{ActiveTurnStatus, TimelineEventId};
use talos_types::talos::TimelineEventEnvelope;
use talos_types::TimelineActiveTurnSnapshot;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::mapping::map_active_turn_snapshot;
use super::types::{
    ApplicationEventStream, TimelineSubscribeCommand, TimelineSubscription,
    TimelineSubscriptionCloseReason, TimelineSubscriptionClosed, TimelineSubscriptionMessage,
};

#[derive(Clone, Default)]
pub struct TimelineSubscriptionService {
    inner: Option<Arc<TimelineEventService>>,
}

impl TimelineSubscriptionService {
    pub fn new(inner: Arc<TimelineEventService>) -> Self {
        Self { inner: Some(inner) }
    }

    pub async fn subscribe(
        &self,
        params: TimelineSubscribeCommand,
    ) -> TalosResult<TimelineSubscription> {
        let live_events = self.subscribe_live_events()?;
        let high_water_mark_event_id = self.latest_event_id(&params.session_id).await?;
        let mut replay = self
            .replay_after(
                &params.session_id,
                params.after_event_id.as_ref(),
                params.replay_limit,
            )
            .await?;
        if let Some(turn_id) = params.turn_id.as_deref() {
            replay.events.retain(|record| record.turn_id == turn_id);
        }
        let has_live_turn = self.has_live_turn(&params.session_id).await?;
        let events = spawn_subscription_stream(
            params.session_id.clone(),
            params.turn_id.clone(),
            replay.events.clone(),
            replay.last_replay_seq,
            live_events,
            has_live_turn,
            params.close_when_idle,
            params.close_on_terminal,
        );
        Ok(TimelineSubscription {
            session_id: params.session_id,
            high_water_mark_event_id,
            replay,
            events,
            has_live_turn,
        })
    }

    pub fn subscribe_live_events(&self) -> TalosResult<ApplicationEventStream> {
        Ok(self.inner_talos()?.subscribe_live_events())
    }

    pub(crate) async fn get_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_store::session::ActiveTurnRecord>> {
        self.inner_talos()?.get_active_turn(session_id).await
    }

    pub async fn get_active_turn_snapshot(
        &self,
        session_id: &str,
    ) -> TalosResult<TimelineActiveTurnSnapshot> {
        let active_turn = self.get_active_turn(session_id).await?;
        Ok(map_active_turn_snapshot(session_id, active_turn))
    }

    pub async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.inner_talos()?.latest_event_id(session_id).await
    }

    pub async fn has_live_turn(&self, session_id: &str) -> TalosResult<bool> {
        Ok(self
            .get_active_turn(session_id)
            .await?
            .is_some_and(|record| {
                matches!(
                    record.status,
                    ActiveTurnStatus::Running
                        | ActiveTurnStatus::WaitingUserAction
                        | ActiveTurnStatus::Cancelling
                )
            }))
    }

    pub async fn run_event_log_gc_loop(&self, shutdown: CancellationToken) {
        match self.inner_talos() {
            Ok(inner) => inner.run_event_log_gc_loop(shutdown).await,
            Err(err) => tracing::error!(error = %err, "event log gc loop could not start"),
        }
    }

    fn inner(&self) -> TalosResult<&Arc<TimelineEventService>> {
        self.inner
            .as_ref()
            .ok_or_else(|| TalosError::InternalError("event service is not configured".to_string()))
    }

    pub(super) fn inner_talos(&self) -> TalosResult<&Arc<TimelineEventService>> {
        self.inner()
    }
}

fn spawn_subscription_stream(
    session_id: String,
    turn_id: Option<String>,
    replay_events: Vec<TimelineEventEnvelope>,
    last_replay_seq: i64,
    mut live_events: ApplicationEventStream,
    has_live_turn: bool,
    close_when_idle: bool,
    close_on_terminal: bool,
) -> mpsc::Receiver<TimelineSubscriptionMessage> {
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut last_sent_seq = last_replay_seq;
        for record in replay_events {
            if !matches_turn_filter(&record, turn_id.as_deref()) {
                continue;
            }
            last_sent_seq = record.event_seq;
            if tx
                .send(TimelineSubscriptionMessage::Event(record))
                .await
                .is_err()
            {
                return;
            }
        }

        if close_when_idle && !has_live_turn {
            let _ = send_subscription_closed(
                &tx,
                TimelineSubscriptionCloseReason::Completed,
                "event subscription completed",
            )
            .await;
            return;
        }

        loop {
            match live_events.recv().await {
                Ok(record) => {
                    if record.session_id != session_id {
                        continue;
                    }
                    if !matches_turn_filter(&record, turn_id.as_deref()) {
                        continue;
                    }
                    let is_live_metrics = matches!(
                        record.event.payload,
                        talos_types::talos::TalosTimelineEventPayload::TurnMetrics(_)
                    );
                    if !is_live_metrics {
                        let event_seq = record.event_seq;
                        if event_seq <= last_sent_seq {
                            continue;
                        }
                        last_sent_seq = event_seq;
                    }
                    let is_terminal = is_terminal_event(&record);
                    if tx
                        .send(TimelineSubscriptionMessage::Event(record))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if close_on_terminal && is_terminal {
                        let _ = send_subscription_closed(
                            &tx,
                            TimelineSubscriptionCloseReason::Completed,
                            "event subscription completed",
                        )
                        .await;
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        skipped,
                        "timeline subscription lagged; closing stream for explicit recovery"
                    );
                    let _ = send_subscription_closed(
                        &tx,
                        TimelineSubscriptionCloseReason::Lagged,
                        "subscription lagged behind retained live events",
                    )
                    .await;
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    let _ = send_subscription_closed(
                        &tx,
                        TimelineSubscriptionCloseReason::ServerShutdown,
                        "event source closed",
                    )
                    .await;
                    return;
                }
            }
        }
    });
    rx
}

fn matches_turn_filter(record: &TimelineEventEnvelope, turn_id: Option<&str>) -> bool {
    turn_id.is_none_or(|turn_id| record.turn_id == turn_id)
}

async fn send_subscription_closed(
    tx: &mpsc::Sender<TimelineSubscriptionMessage>,
    reason: TimelineSubscriptionCloseReason,
    message: &str,
) -> Result<(), mpsc::error::SendError<TimelineSubscriptionMessage>> {
    tx.send(TimelineSubscriptionMessage::Closed(
        TimelineSubscriptionClosed {
            reason,
            message: message.to_string(),
        },
    ))
    .await
}

fn is_terminal_event(record: &TimelineEventEnvelope) -> bool {
    matches!(
        record.event.payload,
        talos_types::talos::TalosTimelineEventPayload::TurnCompleted(_)
            | talos_types::talos::TalosTimelineEventPayload::TurnError(_)
    )
}

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
