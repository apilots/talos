use std::sync::Arc;

use crate::TimelineEventService;
use talos_agent_api::TimelineEvent;
use talos_error::TalosResult;
use talos_types::MessageQueueChangedNotification;
use tokio::sync::{broadcast, mpsc};

use super::types::{
    TimelineEventStream, TimelineSubscribeCommand, TimelineSubscription,
    TimelineSubscriptionCloseReason, TimelineSubscriptionClosed, TimelineSubscriptionMessage,
};
use super::TimelineEventId;

#[derive(Clone)]
pub struct TimelineSubscriptionService {
    inner: Arc<TimelineEventService>,
}

impl TimelineSubscriptionService {
    pub fn new(inner: Arc<TimelineEventService>) -> Self {
        Self { inner }
    }

    pub async fn subscribe(
        &self,
        params: TimelineSubscribeCommand,
    ) -> TalosResult<TimelineSubscription> {
        self.subscribe_inner(params, None).await
    }

    pub(crate) async fn subscribe_with_queue_changes(
        &self,
        params: TimelineSubscribeCommand,
        queue_changes: broadcast::Receiver<MessageQueueChangedNotification>,
    ) -> TalosResult<TimelineSubscription> {
        self.subscribe_inner(params, Some(queue_changes)).await
    }

    async fn subscribe_inner(
        &self,
        params: TimelineSubscribeCommand,
        queue_changes: Option<broadcast::Receiver<MessageQueueChangedNotification>>,
    ) -> TalosResult<TimelineSubscription> {
        let live_events = self.subscribe_live_events();
        let active_changes = self.inner.active_tx.subscribe();
        let high_water_mark_event_id = self.latest_event_id(&params.session_id).await?;
        let last_consumed_seq = params
            .after_event_id
            .as_ref()
            .map(TimelineEventId::event_seq)
            .transpose()?
            .unwrap_or_default();
        let mut replay = self
            .replay_after_through(
                &params.session_id,
                params.after_event_id.as_ref(),
                params.replay_limit,
                high_water_mark_event_id.as_ref(),
            )
            .await?;
        if let Some(turn_id) = params.turn_id.as_deref() {
            replay
                .events
                .retain(|record| record.event.identity().turn_id == turn_id);
        }
        let has_live_turn = self.has_live_turn(&params.session_id).await?;
        let events = spawn_subscription_stream(SubscriptionStreamParams {
            subscriptions: self.clone(),
            session_id: params.session_id.clone(),
            turn_id: params.turn_id.clone(),
            replay_events: replay.events.clone(),
            replay_next_cursor: replay.next_cursor.clone(),
            replay_limit: params.replay_limit,
            high_water_mark_event_id: high_water_mark_event_id.clone(),
            last_consumed_seq,
            live_events,
            active_changes,
            has_live_turn,
            close_when_idle: params.close_when_idle,
            close_on_terminal: params.close_on_terminal,
            queue_changes,
        });
        Ok(TimelineSubscription {
            subscription_id: format!("sub_{}", uuid::Uuid::new_v4().simple()),
            session_id: params.session_id,
            high_water_mark_event_id,
            replay,
            events,
            has_live_turn,
        })
    }

    pub fn subscribe_live_events(&self) -> TimelineEventStream {
        self.inner.subscribe_live_events()
    }

    pub(crate) async fn display_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_agent_api::TimelineActiveTurn>> {
        self.inner.display_active_turn(session_id).await
    }

    pub async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.inner.latest_event_id(session_id).await
    }

    pub async fn has_live_turn(&self, session_id: &str) -> TalosResult<bool> {
        Ok(self.inner.display_active_turn(session_id).await?.is_some())
    }

    pub(super) fn event_service(&self) -> &TimelineEventService {
        &self.inner
    }
}

struct SubscriptionStreamParams {
    subscriptions: TimelineSubscriptionService,
    session_id: String,
    turn_id: Option<String>,
    replay_events: Vec<TimelineEvent>,
    replay_next_cursor: Option<String>,
    replay_limit: usize,
    high_water_mark_event_id: Option<TimelineEventId>,
    last_consumed_seq: i64,
    live_events: TimelineEventStream,
    active_changes: broadcast::Receiver<talos_agent_api::SessionActiveTurnChangedNotification>,
    has_live_turn: bool,
    close_when_idle: bool,
    close_on_terminal: bool,
    queue_changes: Option<broadcast::Receiver<MessageQueueChangedNotification>>,
}

fn spawn_subscription_stream(
    params: SubscriptionStreamParams,
) -> mpsc::Receiver<TimelineSubscriptionMessage> {
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let SubscriptionStreamParams {
            subscriptions,
            session_id,
            turn_id,
            replay_events,
            replay_next_cursor,
            replay_limit,
            high_water_mark_event_id,
            last_consumed_seq,
            mut live_events,
            mut active_changes,
            has_live_turn,
            close_when_idle,
            close_on_terminal,
            mut queue_changes,
        } = params;
        let mut last_sent_seq = last_consumed_seq;
        let mut replay_events = replay_events;
        let mut replay_next_cursor = replay_next_cursor;
        loop {
            for record in replay_events {
                if send_record(
                    &tx,
                    record,
                    turn_id.as_deref(),
                    close_on_terminal,
                    &mut last_sent_seq,
                )
                .await
                .is_break()
                {
                    return;
                }
            }

            let Some(next_cursor) = replay_next_cursor.take() else {
                break;
            };
            let cursor = TimelineEventId::from(next_cursor);
            let next_page = match subscriptions
                .replay_after_through(
                    &session_id,
                    Some(&cursor),
                    replay_limit,
                    high_water_mark_event_id.as_ref(),
                )
                .await
            {
                Ok(page) => page,
                Err(error) => {
                    tracing::error!(
                        session_id = %session_id,
                        error = %error,
                        "timeline subscription could not drain replay backlog"
                    );
                    let _ = send_subscription_closed(
                        &tx,
                        TimelineSubscriptionCloseReason::ServerShutdown,
                        "event subscription replay failed",
                    )
                    .await;
                    return;
                }
            };
            replay_events = next_page.events;
            replay_next_cursor = next_page.next_cursor;
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
            tokio::select! {
            change = active_changes.recv() => {
                match change {
                    Ok(change) if change.session_id == session_id => {
                        if tx.send(TimelineSubscriptionMessage::ActiveTurnChanged(change)).await.is_err() { return; }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        let _ = send_subscription_closed(&tx, TimelineSubscriptionCloseReason::Lagged, "active Turn display requires recovery").await;
                        return;
                    }
                }
            },
            timeline_event = live_events.recv() => match timeline_event {
                Ok(record) => {
                    if record.event.identity().session_id != session_id || record.event_seq <= last_sent_seq {
                        continue;
                    }
                    if record.event_seq == last_sent_seq + 1 {
                        if send_record(&tx, record, turn_id.as_deref(), close_on_terminal, &mut last_sent_seq).await.is_break() { return; }
                        continue;
                    }
                    // Independent product writers may publish out of order; the durable log owns order.
                    let through = TimelineEventId::from(record.event_id);
                    loop {
                        let after = (last_sent_seq > 0).then(|| TimelineEventId::from_seq(last_sent_seq));
                        let page = match subscriptions.replay_after_through(&session_id, after.as_ref(), replay_limit, Some(&through)).await {
                            Ok(page) => page,
                            Err(error) => {
                                talos_logging::report_error(&error);
                                let _ = send_subscription_closed(&tx, TimelineSubscriptionCloseReason::ServerShutdown, "event subscription replay failed").await;
                                return;
                            }
                        };
                        for record in page.events {
                            if send_record(&tx, record, turn_id.as_deref(), close_on_terminal, &mut last_sent_seq).await.is_break() { return; }
                        }
                        if page.next_cursor.is_none() { break; }
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
            },
            queue_change = recv_queue_change(&mut queue_changes), if queue_changes.is_some() => {
                match queue_change {
                    Ok(change) => {
                        if change.session_id != session_id {
                            continue;
                        }
                        if tx
                            .send(TimelineSubscriptionMessage::MessageQueueChanged(change))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            skipped,
                            "message queue subscription lagged; closing stream for explicit refresh"
                        );
                        let _ = send_subscription_closed(
                            &tx,
                            TimelineSubscriptionCloseReason::Lagged,
                            "subscription lagged behind message queue changes",
                        )
                        .await;
                        return;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        let _ = send_subscription_closed(
                            &tx,
                            TimelineSubscriptionCloseReason::ServerShutdown,
                            "message queue event source closed",
                        )
                        .await;
                        return;
                    }
                }
            }
            }
        }
    });
    rx
}

async fn send_record(
    tx: &mpsc::Sender<TimelineSubscriptionMessage>,
    record: TimelineEvent,
    turn_id: Option<&str>,
    close_on_terminal: bool,
    last_sent_seq: &mut i64,
) -> std::ops::ControlFlow<()> {
    use std::ops::ControlFlow;
    if record.event_seq <= *last_sent_seq {
        return ControlFlow::Continue(());
    }
    *last_sent_seq = record.event_seq;
    if !matches_turn_filter(&record, turn_id) {
        return ControlFlow::Continue(());
    }
    let terminal = is_terminal_event(&record);
    if tx
        .send(TimelineSubscriptionMessage::Event(Box::new(record)))
        .await
        .is_err()
    {
        return ControlFlow::Break(());
    }
    if close_on_terminal && terminal {
        let _ = send_subscription_closed(
            tx,
            TimelineSubscriptionCloseReason::Completed,
            "event subscription completed",
        )
        .await;
        return ControlFlow::Break(());
    }
    ControlFlow::Continue(())
}

async fn recv_queue_change(
    receiver: &mut Option<broadcast::Receiver<MessageQueueChangedNotification>>,
) -> Result<MessageQueueChangedNotification, broadcast::error::RecvError> {
    receiver
        .as_mut()
        .expect("queue change branch requires a receiver")
        .recv()
        .await
}

fn matches_turn_filter(record: &TimelineEvent, turn_id: Option<&str>) -> bool {
    turn_id.is_none_or(|turn_id| record.event.identity().turn_id == turn_id)
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

fn is_terminal_event(record: &TimelineEvent) -> bool {
    matches!(
        record.event.agent_event().map(|event| &event.payload),
        Some(
            talos_agent_api::AgentEventPayload::TurnCompleted(_)
                | talos_agent_api::AgentEventPayload::TurnFailed(_)
        )
    )
}

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
