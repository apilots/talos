//! Serialize the original live notifications to the connection queue.
use super::OutgoingMessage;
use talos_app_protocol::events::{
    timeline_event_method, SubscriptionTerminatedNotification, SubscriptionTerminatedReason,
    TimelineEventNotification,
};
use talos_app_protocol::jsonrpc::JsonRpcNotification;
use talos_error::TalosError;
use talos_logging::report_error;
use talos_service::TimelineSubscriptionCloseReason;
use tokio::sync::mpsc;

pub(super) async fn send_active_turn_changed(
    change: talos_agent_api::SessionActiveTurnChangedNotification,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let params = serde_json::to_value(change).map_err(|error| {
        report_error(&TalosError::internal_with_source(
            "Serialize active Turn display; technical details were withheld.",
            error,
        ))
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            "session/activeTurnChanged",
            Some(params),
        )))
        .await
        .map_err(|_| ())
}

pub(super) async fn send_message_queue_changed(
    change: talos_types::MessageQueueChangedNotification,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let params = serde_json::to_value(change).map_err(|err| {
        report_error(&TalosError::internal_with_source(
            "failed to serialize message queue change",
            err,
        ));
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            "messageQueue/changed",
            Some(params),
        )))
        .await
        .map_err(|_| ())
}

pub(super) fn map_subscription_close_reason(
    reason: TimelineSubscriptionCloseReason,
) -> SubscriptionTerminatedReason {
    match reason {
        TimelineSubscriptionCloseReason::Completed => SubscriptionTerminatedReason::Completed,
        TimelineSubscriptionCloseReason::ServerShutdown => {
            SubscriptionTerminatedReason::ServerShutdown
        }
        TimelineSubscriptionCloseReason::Lagged => SubscriptionTerminatedReason::Lagged,
    }
}

pub(super) async fn send_event(
    subscription_id: &str,
    record: talos_agent_api::TimelineEvent,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let method = timeline_event_method(&record.event);
    let params = serde_json::to_value(TimelineEventNotification {
        subscription_id: subscription_id.to_string(),
        event: record,
    })
    .map_err(|err| {
        report_error(&TalosError::internal_with_source(
            "failed to serialize app-server event",
            err,
        ));
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            method,
            Some(params),
        )))
        .await
        .map_err(|_| ())
}

pub(super) async fn send_subscription_terminated(
    subscription_id: &str,
    session_id: &str,
    reason: SubscriptionTerminatedReason,
    message: &str,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let params = serde_json::to_value(SubscriptionTerminatedNotification {
        subscription_id: subscription_id.to_string(),
        session_id: session_id.to_string(),
        reason,
        message: message.to_string(),
    })
    .map_err(|err| {
        report_error(&TalosError::internal_with_source(
            "failed to serialize app-server subscription termination",
            err,
        ));
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            "subscription/terminated",
            Some(params),
        )))
        .await
        .map_err(|_| ())
}
