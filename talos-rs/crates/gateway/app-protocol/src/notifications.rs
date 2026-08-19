use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::events::{SubscriptionTerminatedEnvelope, TimelineSubscriptionNotification};
use crate::version::ProtocolVersion;
use talos_types::MessageQueueChangedNotification;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ServerNotification {
    #[serde(rename = "initialized")]
    Initialized(InitializedNotification),
    #[serde(rename = "timeline.turn.started")]
    TimelineTurnStarted(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.item.started")]
    TimelineItemStarted(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.item.delta")]
    TimelineItemDelta(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.item.completed")]
    TimelineItemCompleted(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.assistant_text.committed")]
    TimelineAssistantTextCommitted(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.turn.metrics")]
    TimelineTurnMetrics(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.turn.completed")]
    TimelineTurnCompleted(TimelineSubscriptionNotification),
    #[serde(rename = "timeline.turn.error")]
    TimelineTurnError(TimelineSubscriptionNotification),
    #[serde(rename = "subscription.terminated")]
    SubscriptionTerminated(SubscriptionTerminatedEnvelope),
    #[serde(rename = "messageQueue/changed")]
    MessageQueueChanged(MessageQueueChangedNotification),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct InitializedNotification {
    pub protocol_version: ProtocolVersion,
}
