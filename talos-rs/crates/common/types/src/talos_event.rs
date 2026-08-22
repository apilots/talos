use crate::talos::{AgentEvent, AgentEventPayload};

impl AgentEvent {
    /// Return the request that owns this timeline event.
    pub fn request_id(&self) -> &str {
        match &self.payload {
            AgentEventPayload::TurnStarted(payload) => &payload.request_id,
            AgentEventPayload::ItemStarted(payload) => &payload.request_id,
            AgentEventPayload::ItemDelta(payload) => &payload.request_id,
            AgentEventPayload::ItemCompleted(payload) => &payload.request_id,
            AgentEventPayload::AssistantTextCommitted(payload) => &payload.request_id,
            AgentEventPayload::TurnMetrics(payload) => &payload.request_id,
            AgentEventPayload::TurnCompleted(payload) => &payload.request_id,
            AgentEventPayload::TurnError(payload) => &payload.request_id,
        }
    }
}
