//! Bridge product Timeline subscriptions to the embedded SDK streams.
use super::*;

impl EmbeddedServerClient {
    pub(super) async fn open_embedded_timeline_stream(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription> {
        let subscription = self
            .runtime
            .service
            .open_timeline_subscription(&self.actor, request)
            .await?;
        let response = subscription.response();
        let subscription_id = response.subscription_id.clone();
        let mut events = subscription.events;
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let (message_queue_tx, message_queue_rx) = tokio::sync::mpsc::channel(256);
        let task = tokio::spawn(async move {
            while let Some(message) = events.recv().await {
                match message {
                    // Match the App Server SDK's timeline-only contract; callers
                    // read volatile display state through get_active_turn.
                    TimelineSubscriptionMessage::ActiveTurnChanged(_) => {}
                    TimelineSubscriptionMessage::Event(event) => {
                        if tx.send(*event).await.is_err() {
                            return;
                        }
                    }
                    TimelineSubscriptionMessage::MessageQueueChanged(change) => {
                        if message_queue_tx.send(change).await.is_err() {
                            return;
                        }
                    }
                    TimelineSubscriptionMessage::Closed(_) => return,
                }
            }
        });
        if let Some(previous) = self.subscriptions.insert(subscription_id, task) {
            previous.abort();
            let _ = previous.await;
        }
        Ok(TimelineStreamSubscription {
            response,
            stream: TimelineEventStream::new(rx),
            message_queue_changes: MessageQueueChangeStream::new(message_queue_rx),
        })
    }

    pub(super) async fn unsubscribe_embedded_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse> {
        let unsubscribed = if let Some(task) = self.subscriptions.remove(&request.subscription_id) {
            task.abort();
            let _ = task.await;
            true
        } else {
            false
        };
        Ok(EventUnsubscribeResponse {
            subscription_id: request.subscription_id,
            unsubscribed,
        })
    }
}
