use super::*;

pub(super) async fn dispatch_application_event(
    subscription_senders: &Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    pending_subscription_events: &Arc<Mutex<HashMap<String, Vec<TimelineEventEnvelope>>>>,
    subscription_id: &str,
    event: TimelineEventEnvelope,
) {
    let sender = {
        let streams = subscription_senders.lock().await;
        let Some(stream) = streams.get(subscription_id) else {
            pending_subscription_events
                .lock()
                .await
                .entry(subscription_id.to_string())
                .or_default()
                .push(event);
            return;
        };
        if event.session_id != stream.session_id {
            return;
        }
        stream.sender.clone()
    };
    if sender.try_send(event).is_err() {
        subscription_senders.lock().await.remove(subscription_id);
    }
}
