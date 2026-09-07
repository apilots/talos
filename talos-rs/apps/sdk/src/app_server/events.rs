use super::*;

pub(super) async fn dispatch_timeline_event(
    subscription_senders: &Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    pending_subscription_events: &Arc<Mutex<HashMap<String, Vec<TimelineEvent>>>>,
    subscription_id: &str,
    event: TimelineEvent,
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
        if event.event.identity().session_id != stream.session_id {
            return;
        }
        stream.sender.clone()
    };
    if sender.try_send(event).is_err() {
        subscription_senders.lock().await.remove(subscription_id);
    }
}

pub(super) async fn dispatch_message_queue_change(
    subscription_senders: &Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    change: talos_types::MessageQueueChangedNotification,
) {
    subscription_senders.lock().await.retain(|_, stream| {
        stream.session_id != change.session_id
            || stream.message_queue_sender.try_send(change.clone()).is_ok()
    });
}
