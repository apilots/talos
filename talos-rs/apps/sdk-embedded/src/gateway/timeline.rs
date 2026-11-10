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
        let (automation_tx, automation_rx) = tokio::sync::mpsc::channel(256);
        let mut subscriptions = self
            .subscriptions
            .lock()
            .expect("embedded subscription lock poisoned");
        let subscriptions = subscriptions.as_mut().ok_or_else(|| {
            talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "The embedded client is shut down.",
            )
        })?;
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
                    TimelineSubscriptionMessage::AutomationChanged(change) => {
                        if automation_tx.send(change).await.is_err() {
                            return;
                        }
                    }
                    TimelineSubscriptionMessage::Closed(_) => return,
                }
            }
        });
        if let Some(previous) = subscriptions.insert(subscription_id, task) {
            previous.abort();
        }
        Ok(TimelineStreamSubscription {
            response,
            stream: TimelineEventStream::new(rx),
            message_queue_changes: MessageQueueChangeStream::new(message_queue_rx),
            automation_changes: AutomationChangeStream::new(automation_rx),
        })
    }

    pub(super) async fn unsubscribe_embedded_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse> {
        let task = self
            .subscriptions
            .lock()
            .expect("embedded subscription lock poisoned")
            .as_mut()
            .and_then(|subscriptions| subscriptions.remove(&request.subscription_id));
        let unsubscribed = if let Some(task) = task {
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
