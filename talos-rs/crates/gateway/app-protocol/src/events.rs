use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_types::talos::{TalosTimelineEventType, TimelineEventEnvelope};
use ts_rs::TS;

pub type EventSubscriptionId = String;
pub type ApplicationEvent = TimelineEventEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct TimelineSubscriptionNotification {
    pub subscription_id: EventSubscriptionId,
    pub event: TimelineEventEnvelope,
}

pub fn canonical_method(event_type: &TalosTimelineEventType) -> &'static str {
    match event_type {
        TalosTimelineEventType::TurnStarted => "timeline.turn.started",
        TalosTimelineEventType::ItemStarted => "timeline.item.started",
        TalosTimelineEventType::ItemDelta => "timeline.item.delta",
        TalosTimelineEventType::ItemCompleted => "timeline.item.completed",
        TalosTimelineEventType::AssistantTextCommitted => "timeline.assistant_text.committed",
        TalosTimelineEventType::TurnMetrics => "timeline.turn.metrics",
        TalosTimelineEventType::TurnCompleted => "timeline.turn.completed",
        TalosTimelineEventType::TurnError => "timeline.turn.error",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct SubscriptionTerminatedEnvelope {
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

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
