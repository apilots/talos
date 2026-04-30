use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::events::{ApplicationEventEnvelope, SubscriptionTerminatedEnvelope};
use crate::version::ProtocolVersion;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum ServerNotification {
    #[serde(rename = "initialized")]
    Initialized(InitializedNotification),
    #[serde(rename = "message.started")]
    MessageStarted(ApplicationEventEnvelope),
    #[serde(rename = "message.reasoning.delta")]
    MessageReasoningDelta(ApplicationEventEnvelope),
    #[serde(rename = "message.execution.summary")]
    MessageExecutionSummary(ApplicationEventEnvelope),
    #[serde(rename = "message.output.delta")]
    MessageOutputDelta(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.started")]
    MessageToolCallStarted(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.args_delta")]
    MessageToolCallArgsDelta(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.queued")]
    MessageToolCallQueued(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.executing")]
    MessageToolCallExecuting(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.progress_delta")]
    MessageToolCallProgressDelta(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.completed")]
    MessageToolCallCompleted(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.pending_approval")]
    MessageToolCallPendingApproval(ApplicationEventEnvelope),
    #[serde(rename = "message.tool_call.pending_dynamic_result")]
    MessageToolCallPendingDynamicResult(ApplicationEventEnvelope),
    #[serde(rename = "message.completed")]
    MessageCompleted(ApplicationEventEnvelope),
    #[serde(rename = "message.error")]
    MessageError(ApplicationEventEnvelope),
    #[serde(rename = "subscription.terminated")]
    SubscriptionTerminated(SubscriptionTerminatedEnvelope),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct InitializedNotification {
    pub protocol_version: ProtocolVersion,
}
