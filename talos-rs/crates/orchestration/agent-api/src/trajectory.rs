//! Optional Session trajectory reads. These objects never extend canonical events.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::ApiError;
use talos_types::{AgentTurnRef, JsonContentValue, ModelSpeed, ReasoningEffort, TokenUsage};
use ts_rs::TS;

/// Reference to content frozen inside one private execution-history supplement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PayloadRef {
    pub payload_id: String,
    /// True/false means observed complete/partial; null means unknown.
    pub complete: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryCapabilitiesResponse {
    pub supported: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadTurnRequest {
    pub identity: AgentTurnRef,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClockSegment {
    pub segment_id: String,
    pub source_node_id: String,
    #[ts(type = "number")]
    pub started_at: i64,
    pub relation: ClockRelation,
    pub observed_through_offset_ms: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ClockRelation {
    TurnStart,
    Resumed,
    Unanchored,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionInterval {
    pub segment_id: String,
    pub started_offset_ms: f64,
    pub ended_offset_ms: Option<f64>,
    pub elapsed_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallTiming {
    pub interval: ExecutionInterval,
    pub first_output_offset_ms: Option<f64>,
    pub last_output_offset_ms: Option<f64>,
    pub ttft_ms: Option<f64>,
    pub generation_ms: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ModelCallState {
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallRecord {
    pub call_record_id: String,
    pub ordinal: u32,
    pub observed_state: ModelCallState,
    pub timing: Option<ModelCallTiming>,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
    pub error: Option<ApiError>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ModelRequestPurpose {
    Working,
    FinalAnswer,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRequestTrace {
    pub model_request_id: String,
    pub ordinal: u32,
    pub purpose: ModelRequestPurpose,
    pub connection_id: String,
    pub model_id: String,
    pub call_records: Vec<ModelCallRecord>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ToolIntervalKind {
    Execution,
    WaitingForInput,
    Queue,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolInterval {
    pub kind: ToolIntervalKind,
    pub interval: ExecutionInterval,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryTool {
    pub call_id: String,
    pub source_model_request_id: Option<String>,
    pub source_call_record_id: Option<String>,
    pub intervals: Option<Vec<ToolInterval>>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TrajectoryEntry {
    ModelRequest { request: ModelRequestTrace },
    Tool { tool: TrajectoryTool },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadTurnResponse {
    pub identity: AgentTurnRef,
    pub clock_segments: Vec<ClockSegment>,
    pub data: Option<Vec<TrajectoryEntry>>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadModelRequestRequest {
    pub identity: AgentTurnRef,
    pub model_request_id: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCallOutput {
    pub call_record_id: String,
    pub response: Option<PayloadRef>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadModelRequestResponse {
    pub identity: AgentTurnRef,
    pub request: ModelRequestTrace,
    pub prompt: Option<PayloadRef>,
    pub tools: Option<PayloadRef>,
    pub parameters: Option<PayloadRef>,
    pub outputs: Vec<ModelCallOutput>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadToolRequest {
    pub identity: AgentTurnRef,
    pub call_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum ToolInputUsageKind {
    Direct,
    Transformed,
    NotIncluded,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolInputUsage {
    pub model_request_id: String,
    pub message_index: Option<u32>,
    pub usage: ToolInputUsageKind,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadToolResponse {
    pub identity: AgentTurnRef,
    pub tool: TrajectoryTool,
    pub prepared_result: Option<PayloadRef>,
    pub usages: Option<Vec<ToolInputUsage>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotAttachmentKind {
    Image,
    File,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotAvailability {
    Available,
    NotRetained,
    Expired,
    Redacted,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotAttachment {
    pub file_id: Option<String>,
    pub filename: Option<String>,
    pub kind: Option<SnapshotAttachmentKind>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<f64>,
    pub availability: SnapshotAvailability,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SnapshotCacheControl {
    None,
    Ephemeral,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: JsonContentValue,
    pub arguments_text: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotInputKind {
    System,
    User,
    Context,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotInputSource {
    User,
    Prompt,
    Runtime,
    Workspace,
    Skills,
    Memory,
    Mixed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotInputAdmission {
    Introduced,
    Carried,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotInputOrigin {
    pub kind: SnapshotInputKind,
    pub source: SnapshotInputSource,
    pub admission: SnapshotInputAdmission,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotRole {
    System,
    User,
    Assistant,
    Tool,
}
/// Public reasoning replay only; provider signatures and encrypted data are excluded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SnapshotReasoningPart {
    Text { text: String },
    Summary { text: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotMessage {
    pub index: u32,
    pub role: SnapshotRole,
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<Vec<SnapshotReasoningPart>>,
    pub name: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_calls: Option<Vec<SnapshotToolCall>>,
    pub attachments: Option<Vec<SnapshotAttachment>>,
    pub cache_control: Option<SnapshotCacheControl>,
    pub input_origin: Option<SnapshotInputOrigin>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Option<JsonContentValue>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SnapshotToolChoice {
    Auto,
    None,
    Required,
    Named { name: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedRequestOptions {
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub stop: Option<Vec<String>>,
    pub parallel_tool_calls: Option<bool>,
    pub tool_choice: Option<SnapshotToolChoice>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub speed: Option<ModelSpeed>,
    pub request_parameters: std::collections::BTreeMap<String, JsonContentValue>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ResponsePart {
    Text { text: String },
    Reasoning { text: String },
    ToolCall { call: SnapshotToolCall },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TrajectoryPayload {
    Prompt {
        messages: Vec<SnapshotMessage>,
    },
    Tools {
        definitions: Vec<SnapshotToolDefinition>,
    },
    Parameters {
        options: NormalizedRequestOptions,
    },
    Response {
        parts: Vec<ResponsePart>,
    },
    PreparedResult {
        message: SnapshotMessage,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadPayloadRequest {
    pub identity: AgentTurnRef,
    pub payload_id: String,
    pub cursor: Option<String>,
    pub limit_bytes: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrajectoryReadPayloadResponse {
    pub payload_id: String,
    pub encoding: TrajectoryPayloadEncoding,
    pub data: String,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum TrajectoryPayloadEncoding {
    Utf8Json,
}

/// Private execution-boundary facts attached to the original TurnStarted event.
/// This is neither a public History DTO nor a second execution lifecycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionHistorySupplement {
    pub identity: AgentTurnRef,
    pub clock_segments: Vec<ClockSegment>,
    pub requests: Vec<TrajectoryReadModelRequestResponse>,
    pub tools: Vec<TrajectoryReadToolResponse>,
    pub payloads: std::collections::BTreeMap<String, TrajectoryPayload>,
}
