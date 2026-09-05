use super::*;

pub(super) fn spawn_stdout_reader(
    stdout: ChildStdout,
    pending_responses: Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>,
    subscription_senders: Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    pending_subscription_events: Arc<Mutex<HashMap<String, Vec<TimelineEvent>>>>,
    events_tx: mpsc::Sender<TimelineEvent>,
    notifications_tx: mpsc::Sender<TimelineEventNotification>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            let read_result = reader.read_line(&mut line).await;
            let bytes = match read_result {
                Ok(bytes) => bytes,
                Err(error) => {
                    response::fail_pending_responses(&pending_responses, error.to_string()).await;
                    return;
                }
            };
            if bytes == 0 {
                response::fail_pending_responses(
                    &pending_responses,
                    "app-server stdout closed".to_string(),
                )
                .await;
                return;
            }

            let message = match serde_json::from_str::<JsonRpcMessage>(line.trim_end()) {
                Ok(message) => message,
                Err(error) => {
                    response::fail_pending_responses(&pending_responses, error.to_string()).await;
                    return;
                }
            };

            match message {
                JsonRpcMessage::Response(response) => {
                    if let RequestId::Number(id) = response.id {
                        if let Some(sender) = pending_responses.lock().await.remove(&id) {
                            let _ = sender.send(response::decode_api_response(response.result));
                        }
                    }
                }
                JsonRpcMessage::Error(error) => {
                    if let Some(RequestId::Number(id)) = error.id {
                        if let Some(sender) = pending_responses.lock().await.remove(&id) {
                            let _ = sender.send(Err(response::map_app_server_error(error.error)));
                        }
                    }
                }
                JsonRpcMessage::Notification(notification) => {
                    if let Ok(Some(terminated)) =
                        response::decode_subscription_terminated(&notification)
                    {
                        subscription_senders
                            .lock()
                            .await
                            .remove(&terminated.subscription_id);
                        pending_subscription_events
                            .lock()
                            .await
                            .remove(&terminated.subscription_id);
                        continue;
                    }
                    if let Ok(Some(change)) = response::decode_message_queue_changed(&notification)
                    {
                        dispatch_message_queue_change(&subscription_senders, change).await;
                        continue;
                    }
                    let Ok(Some(notification)) = response::decode_event_notification(notification)
                    else {
                        continue;
                    };
                    let subscription_id = notification.subscription_id.clone();
                    let event = notification.event.clone();
                    dispatch_timeline_event(
                        &subscription_senders,
                        &pending_subscription_events,
                        &subscription_id,
                        event.clone(),
                    )
                    .await;
                    match notifications_tx.try_send(notification) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return,
                    }
                    match events_tx.try_send(event) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return,
                    }
                }
                JsonRpcMessage::Request(_) => {}
            }
        }
    });
}
