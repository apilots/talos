use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::events::{SubscriptionTerminatedNotification, TimelineEventNotification};
use crate::version::ProtocolVersion;
use talos_types::MessageQueueChangedNotification;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ServerNotification {
    #[serde(rename = "initialized")]
    Initialized(InitializedNotification),
    #[serde(rename = "timeline/turn/started")]
    TimelineTurnStarted(TimelineEventNotification),
    #[serde(rename = "timeline/message/started")]
    TimelineMessageStarted(TimelineEventNotification),
    #[serde(rename = "timeline/message/block/started")]
    TimelineMessageBlockStarted(TimelineEventNotification),
    #[serde(rename = "timeline/message/block/delta")]
    TimelineMessageBlockDelta(TimelineEventNotification),
    #[serde(rename = "timeline/message/block/completed")]
    TimelineMessageBlockCompleted(TimelineEventNotification),
    #[serde(rename = "timeline/message/completed")]
    TimelineMessageCompleted(TimelineEventNotification),
    #[serde(rename = "timeline/waiting_for_input")]
    TimelineWaitingForInput(TimelineEventNotification),
    #[serde(rename = "timeline/metrics/updated")]
    TimelineMetricsUpdated(TimelineEventNotification),
    #[serde(rename = "timeline/turn/completed")]
    TimelineTurnCompleted(TimelineEventNotification),
    #[serde(rename = "timeline/turn/failed")]
    TimelineTurnFailed(TimelineEventNotification),
    #[serde(rename = "subscription/terminated")]
    SubscriptionTerminated(SubscriptionTerminatedNotification),
    #[serde(rename = "messageQueue/changed")]
    MessageQueueChanged(MessageQueueChangedNotification),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct InitializedNotification {
    pub protocol_version: ProtocolVersion,
}
