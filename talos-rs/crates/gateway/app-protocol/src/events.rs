use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_types::talos::{TalosEvent, TalosEventType};
use ts_rs::TS;

pub type EventSubscriptionId = String;
pub type ApplicationEvent = TalosEvent;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
pub struct ApplicationEventEnvelope {
    pub subscription_id: EventSubscriptionId,
    pub event_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub event: ApplicationEvent,
}

pub fn canonical_method(event_type: &TalosEventType) -> &'static str {
    match event_type {
        TalosEventType::MessageStarted => "message.started",
        TalosEventType::MessageReasoningDelta => "message.reasoning.delta",
        TalosEventType::MessageExecutionSummary => "message.execution.summary",
        TalosEventType::MessageOutputDelta => "message.output.delta",
        TalosEventType::MessageToolCallStarted => "message.tool_call.started",
        TalosEventType::MessageToolCallArgsDelta => "message.tool_call.args_delta",
        TalosEventType::MessageToolCallQueued => "message.tool_call.queued",
        TalosEventType::MessageToolCallExecuting => "message.tool_call.executing",
        TalosEventType::MessageToolCallProgressDelta => "message.tool_call.progress_delta",
        TalosEventType::MessageToolCallCompleted => "message.tool_call.completed",
        TalosEventType::MessageToolCallPendingApproval => "message.tool_call.pending_approval",
        TalosEventType::MessageToolCallPendingDynamicResult => {
            "message.tool_call.pending_dynamic_result"
        }
        TalosEventType::MessageCompleted => "message.completed",
        TalosEventType::MessageError => "message.error",
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
mod tests {
    use super::*;

    #[test]
    fn canonical_method_matches_talos_event_type() {
        assert_eq!(
            canonical_method(&TalosEventType::MessageOutputDelta),
            "message.output.delta"
        );
    }
}
