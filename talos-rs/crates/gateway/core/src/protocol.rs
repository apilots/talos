//! Talos Gateway - Orchestration Protocol
//!
//! 定义 Gateway 与 Orchestration 层之间的调用协议

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
use talos_types::{AgentId, SessionId, UserId, ChannelId};
use tokio::sync::mpsc;
use talos_error::TalosResult;

// ============================================================================
// Request/Response 基础类型
// ============================================================================

/// Gateway → Orchestration 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRequest {
    /// 请求 ID（用于幂等性和追踪）
    pub request_id: String,
    /// 请求类型
    pub kind: RequestKind,
    /// 会话上下文
    pub context: SessionContext,
    /// 请求负载
    pub payload: RequestPayload,
    /// 元数据
    #[serde(default)]
    pub metadata: RequestMetadata,
}

/// 请求类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    /// 用户消息处理
    ChatMessage,
    /// 创建新会话
    CreateSession,
    /// 获取会话状态
    GetSession,
    /// 销毁会话
    DestroySession,
    /// 工具调用结果
    ToolResult,
    /// Agent 内部事件
    AgentEvent,
}

/// 会话上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    /// 会话 ID
    pub session_id: SessionId,
    /// 用户 ID
    pub user_id: UserId,
    /// 渠道 ID
    pub channel_id: ChannelId,
    /// Agent ID（路由结果）
    #[serde(default)]
    pub agent_id: Option<AgentId>,
    /// 对话线程 ID
    #[serde(default)]
    pub thread_id: Option<String>,
}

/// 请求负载
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum RequestPayload {
    /// 聊天消息
    ChatMessage(ChatMessageRequest),
    /// 创建会话
    CreateSession(CreateSessionRequest),
    /// 工具结果
    ToolResult(ToolResultRequest),
    /// Agent 事件
    AgentEvent(AgentEventRequest),
}

/// 聊天消息请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageRequest {
    /// 消息内容
    pub content: Vec<MessageContent>,
    /// 附件
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// 是否流式响应
    #[serde(default = "default_true")]
    pub stream: bool,
    /// 消息时间戳
    #[serde(default = "SystemTime::now")]
    pub timestamp: SystemTime,
}

/// 消息内容类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum MessageContent {
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "image")]
    Image { url: String, mime_type: String },
    #[serde(rename = "audio")]
    Audio { data: Vec<u8>, mime_type: String },
    #[serde(rename = "command")]
    Command { name: String, args: HashMap<String, String> },
}

/// 附件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub size: u64,
    pub url: Option<String>,
    pub data: Option<Vec<u8>>,
}

/// 创建会话请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    /// 会话元数据
    pub metadata: SessionMetadata,
    /// 初始消息（可选）
    #[serde(default)]
    pub initial_message: Option<ChatMessageRequest>,
}

/// 会话元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// 会话名称
    #[serde(default)]
    pub name: Option<String>,
    /// 渠道特定数据
    #[serde(default)]
    pub channel_data: HashMap<String, String>,
    /// 用户偏好
    #[serde(default)]
    pub preferences: HashMap<String, String>,
}

/// 工具结果请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRequest {
    /// 工具调用 ID
    pub call_id: String,
    /// 工具名称
    pub tool_name: String,
    /// 执行结果
    pub result: ToolResult,
}

/// 工具执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ToolResult {
    #[serde(rename = "success")]
    Success { output: String },
    #[serde(rename = "error")]
    Error { error_code: String, message: String },
}

/// Agent 事件请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEventRequest {
    /// 事件类型
    pub event_type: String,
    /// 事件数据
    pub data: serde_json::Value,
}

// ============================================================================
// Response 类型
// ============================================================================

/// Orchestration → Gateway 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResponse {
    /// 对应的请求 ID
    pub request_id: String,
    /// 响应状态
    pub status: ResponseStatus,
    /// 响应负载
    #[serde(default)]
    pub payload: Option<ResponsePayload>,
    /// 错误信息（当 status 为 error 时）
    #[serde(default)]
    pub error: Option<ResponseError>,
}

/// 响应状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Error,
    Partial,
    Continued,
}

/// 响应负载
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ResponsePayload {
    /// 聊天响应
    ChatMessage(ChatMessageResponse),
    /// 会话信息
    SessionInfo(SessionInfo),
    /// 工具确认
    ToolAck(ToolAckResponse),
    /// 事件确认
    EventAck(EventAckResponse),
}

/// 聊天响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageResponse {
    /// 响应消息 ID
    pub message_id: String,
    /// 响应内容
    pub content: Vec<ResponseContent>,
    /// 使用统计
    #[serde(default)]
    pub usage: Option<UsageInfo>,
    /// 完成原因
    #[serde(default)]
    pub finish_reason: Option<FinishReason>,
}

/// 响应内容类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ResponseContent {
    #[serde(rename = "text")]
    Text(String),
    #[serde(rename = "image")]
    Image { url: String, mime_type: String },
    #[serde(rename = "tool_call")]
    ToolCall(ToolCallInfo),
    #[serde(rename = "reasoning")]
    Reasoning(String),
}

/// 工具调用信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub call_id: String,
    pub name: String,
    pub arguments: HashMap<String, serde_json::Value>,
}

/// 使用统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

/// 完成原因
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    Error,
    Interrupted,
}

/// 会话信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub created_at: SystemTime,
    pub last_activity: SystemTime,
    pub message_count: u64,
    pub agent_id: AgentId,
    pub status: SessionStatus,
}

/// 会话状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Idle,
    Closed,
    Expired,
}

/// 工具确认响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAckResponse {
    pub call_id: String,
    pub acknowledged: bool,
}

/// 事件确认响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAckResponse {
    pub acknowledged: bool,
}

/// 响应错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: HashMap<String, String>,
}

// ============================================================================
// 流式响应类型
// ============================================================================

/// 流式响应 Chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// 请求 ID
    pub request_id: String,
    /// Chunk 类型
    pub kind: ChunkKind,
    /// Chunk 数据
    pub data: ChunkData,
    /// 序列号（用于排序）
    pub sequence: u64,
}

/// Chunk 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    /// 文本增量
    TextDelta,
    /// 工具调用开始
    ToolCallStart,
    /// 工具调用增量
    ToolCallDelta,
    /// 工具调用结束
    ToolCallEnd,
    /// 推理内容
    Reasoning,
    /// 完成
    Done,
    /// 错误
    Error,
}

/// Chunk 数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ChunkData {
    #[serde(rename = "text_delta")]
    TextDelta(String),
    #[serde(rename = "tool_call_start")]
    ToolCallStart(ToolCallStart),
    #[serde(rename = "tool_call_delta")]
    ToolCallDelta(ToolCallDelta),
    #[serde(rename = "tool_call_end")]
    ToolCallEnd(ToolCallEnd),
    #[serde(rename = "reasoning")]
    Reasoning(String),
    #[serde(rename = "done")]
    Done(FinishReason),
    #[serde(rename = "error")]
    Error(ResponseError),
}

/// 工具调用开始
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStart {
    pub call_id: String,
    pub name: String,
}

/// 工具调用增量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub call_id: String,
    pub arguments_delta: String,
}

/// 工具调用结束
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEnd {
    pub call_id: String,
    pub arguments: String,
}

// ============================================================================
// 辅助类型
// ============================================================================

/// 请求元数据
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestMetadata {
    /// 优先级
    #[serde(default)]
    pub priority: Priority,
    /// 超时时间（毫秒）
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// 追踪 ID
    #[serde(default)]
    pub trace_id: Option<String>,
    /// 是否采样
    #[serde(default)]
    pub sampled: bool,
}

/// 优先级
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

fn default_true() -> bool { true }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_request_serialization() {
        let req = GatewayRequest {
            request_id: "req-123".to_string(),
            kind: RequestKind::ChatMessage,
            context: SessionContext {
                session_id: SessionId::new(),
                user_id: UserId::new("user-1"),
                channel_id: ChannelId::new("telegram"),
                agent_id: Some(AgentId::new("main")),
                thread_id: None,
            },
            payload: RequestPayload::ChatMessage(ChatMessageRequest {
                content: vec![MessageContent::Text("Hello".to_string())],
                attachments: vec![],
                stream: true,
                timestamp: SystemTime::now(),
            }),
            metadata: RequestMetadata::default(),
        };

        let json = serde_json::to_string_pretty(&req).unwrap();
        println!("{}", json);

        let _deserialized: GatewayRequest = serde_json::from_str(&json).unwrap();
    }
}
