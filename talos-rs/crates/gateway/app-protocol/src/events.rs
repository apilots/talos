use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_agent_api::{AgentEventPayload, TimelineEvent};
use ts_rs::TS;

pub type EventSubscriptionId = String;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct TimelineEventNotification {
    pub subscription_id: EventSubscriptionId,
    pub event: TimelineEvent,
}

/// Return the single wire method used by JSON-RPC and SSE for one Agent event.
pub fn timeline_event_method(payload: &AgentEventPayload) -> &'static str {
    match payload {
        AgentEventPayload::TurnStarted(_) => "timeline/turn/started",
        AgentEventPayload::MessageStarted(_) => "timeline/message/started",
        AgentEventPayload::MessageBlockStarted(_) => "timeline/message/block/started",
        AgentEventPayload::MessageBlockDelta(_) => "timeline/message/block/delta",
        AgentEventPayload::MessageBlockCompleted(_) => "timeline/message/block/completed",
        AgentEventPayload::MessageCompleted(_) => "timeline/message/completed",
        AgentEventPayload::WaitingForInput(_) => "timeline/waiting_for_input",
        AgentEventPayload::MetricsUpdated(_) => "timeline/metrics/updated",
        AgentEventPayload::TurnCompleted(_) => "timeline/turn/completed",
        AgentEventPayload::TurnFailed(_) => "timeline/turn/failed",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SubscriptionTerminatedNotification {
    pub subscription_id: EventSubscriptionId,
    pub session_id: String,
    pub reason: SubscriptionTerminatedReason,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigWarningEvent {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct SubscriptionTerminatedEvent {
    pub reason: SubscriptionTerminatedReason,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionTerminatedReason {
    Completed,
    Interrupted,
    ServerShutdown,
    SessionDeleted,
    Lagged,
    SlowConsumer,
}
