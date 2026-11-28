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

pub(super) async fn dispatch_automation_change(
    subscription_senders: &Arc<Mutex<HashMap<String, SubscriptionStream>>>,
    change: talos_types::AutomationChangedNotification,
) {
    subscription_senders.lock().await.retain(|_, stream| {
        stream.session_id != change.session_id
            || stream.automation_sender.try_send(change.clone()).is_ok()
    });
}

impl AppServerClient {
    pub(super) async fn open_product_timeline(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription> {
        let http = self.product_http.as_ref().expect("Product HTTP transport");
        let mut url = http
            .origin
            .join("v1/sessions/")
            .map_err(super::request::http_error)?;
        url.path_segments_mut()
            .map_err(|_| TalosError::internal("Invalid product origin."))?
            .pop_if_empty()
            .push(&request.session_id)
            .push("events");
        {
            let mut query = url.query_pairs_mut();
            if let Some(cursor) = &request.since_event_id {
                query.append_pair("after_event_id", cursor);
            }
            if let Some(turn) = &request.turn_id {
                query.append_pair("turn_id", turn);
            }
            if let Some(limit) = request.replay_limit {
                query.append_pair("limit", &limit.to_string());
            }
            query.append_pair(
                "close_when_idle",
                if request.close_when_idle.unwrap_or(false) {
                    "true"
                } else {
                    "false"
                },
            );
        }
        let response = http
            .send_authorized(http.authorized(http.client.get(url)))
            .await?;
        if !response.status().is_success() {
            let body = response.json().await.map_err(super::request::http_error)?;
            return super::response::decode_api_response(body).and_then(|_| {
                Err(TalosError::new(
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Talos Server subscription was rejected.",
                ))
            });
        }
        let subscription_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::channel(256);
        let (queue_tx, queue_rx) = mpsc::channel(256);
        let (automation_tx, automation_rx) = mpsc::channel(256);
        let task = tokio::spawn(async move {
            let result = read_product_events(response, tx, queue_tx, automation_tx).await;
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
        });
        http.tasks
            .lock()
            .expect("Product subscription tasks lock")
            .insert(subscription_id.clone(), task);
        Ok(TimelineStreamSubscription {
            response: SubscribeTimelineResponse {
                subscription_id,
                session_id: request.session_id,
                high_water_mark_event_id: None,
            },
            stream: TimelineEventStream::new(rx),
            message_queue_changes: MessageQueueChangeStream::new(queue_rx),
            automation_changes: AutomationChangeStream::new(automation_rx),
        })
    }
}

async fn read_product_events(
    response: reqwest::Response,
    events: mpsc::Sender<TimelineEvent>,
    queues: mpsc::Sender<talos_types::MessageQueueChangedNotification>,
    automation: mpsc::Sender<talos_types::AutomationChangedNotification>,
) -> TalosResult<()> {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut pending = Vec::new();
    while let Some(chunk) = stream.next().await {
        pending.extend_from_slice(&chunk.map_err(super::request::http_error)?);
        if pending.len() > 16 * 1024 * 1024 {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "The product event exceeds the subscription limit.",
            ));
        }
        loop {
            let delimiter = pending
                .windows(2)
                .position(|bytes| bytes == b"\n\n")
                .map(|index| (index, 2))
                .or_else(|| {
                    pending
                        .windows(4)
                        .position(|bytes| bytes == b"\r\n\r\n")
                        .map(|index| (index, 4))
                });
            let Some((end, delimiter)) = delimiter else {
                break;
            };
            let frame = String::from_utf8(pending.drain(..end + delimiter).collect())
                .map_err(super::request::http_error)?;
            let mut name = "message";
            let mut data = Vec::new();
            for line in frame.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    name = value.trim_start();
                }
                if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if data.is_empty() {
                continue;
            }
            let data = data.join("\n");
            let delivered = match name {
                "messageQueue/changed" => queues
                    .send(serde_json::from_str(&data).map_err(super::request::http_error)?)
                    .await
                    .is_ok(),
                "automation/changed" => automation
                    .send(serde_json::from_str(&data).map_err(super::request::http_error)?)
                    .await
                    .is_ok(),
                "subscription/terminated" => return Ok(()),
                "session/activeTurnChanged" => true,
                _ => events
                    .send(serde_json::from_str(&data).map_err(super::request::http_error)?)
                    .await
                    .is_ok(),
            };
            if !delivered {
                return Ok(());
            }
        }
    }
    Ok(())
}
