//! Talos LLM provider abstraction.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
pub use talos_types::{ModelRef, ModelSpeed, ReasoningEffort};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::{validate_attachment_usage, Attachment, AttachmentUsage};

/// Unified function call type used across providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

impl FunctionCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }
}

/// Unified function response type used to feed tool outputs back to providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionResponse {
    pub id: String,
    pub name: String,
    pub content: Value,
    pub is_error: bool,
}

impl FunctionResponse {
    pub fn success(id: impl Into<String>, name: impl Into<String>, content: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            content,
            is_error: false,
        }
    }

    pub fn error(id: impl Into<String>, name: impl Into<String>, content: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            content,
            is_error: true,
        }
    }
}

/// Tool definition exposed to model providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Option<Value>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    pub fn validate_arguments(&self, args: &Value) -> Result<(), ToolCallingError> {
        let Some(schema) = self.parameters.as_ref() else {
            return Ok(());
        };

        let Some(required) = schema.get("required").and_then(|v| v.as_array()) else {
            return Ok(());
        };

        let Some(obj) = args.as_object() else {
            return Err(ToolCallingError::InvalidArguments {
                tool: self.name.clone(),
                error: "arguments must be a JSON object".to_string(),
            });
        };

        for field in required {
            if let Some(name) = field.as_str() {
                if !obj.contains_key(name) {
                    return Err(ToolCallingError::InvalidArguments {
                        tool: self.name.clone(),
                        error: format!("missing required field: {}", name),
                    });
                }
            }
        }

        Ok(())
    }
}

/// Tool calling strategy hint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Named { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptCacheSupport {
    Unsupported,
    RequestLevel,
}

/// Unified tool calling error.
#[derive(Debug, Error)]
pub enum ToolCallingError {
    #[error("tool not found: {name}")]
    ToolNotFound { name: String },

    #[error("invalid arguments for tool '{tool}': {error}")]
    InvalidArguments { tool: String, error: String },

    #[error("tool execution failed for '{tool}': {error}")]
    ToolExecutionFailed { tool: String, error: String },

    #[error("provider error: {error}")]
    ProviderError { error: String },
}

/// Executes provider requests and validates inputs before wire encoding.
///
/// Implementations own provider-specific capability and protocol differences.
/// Callers provide provider-neutral requests and must not infer support from a
/// provider name or silently drop unsupported fields.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse>;
    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream>;
    fn default_model(&self) -> String;
    fn prompt_cache_support(&self) -> PromptCacheSupport {
        PromptCacheSupport::Unsupported
    }

    /// Validates binary inputs for one model and returns conservative token bounds.
    ///
    /// Providers must override this method only when their selected wire protocol
    /// supports every accepted attachment. The default rejects all non-empty input.
    fn validate_attachments(
        &self,
        _model: &str,
        attachments: &[Attachment],
    ) -> TalosResult<AttachmentUsage> {
        if attachments.is_empty() {
            return Ok(AttachmentUsage {
                item_tokens: Vec::new(),
            });
        }
        Err(TalosError::new(
            ErrorCode::UnsupportedAttachment,
            "model attachments are unsupported",
        ))
    }

    /// Validates every attached message before a provider constructs a request.
    ///
    /// Provider adapters call this shared entry point and normally override
    /// [`LlmProvider::validate_attachments`] rather than this method.
    fn validate_request_attachments(&self, request: &ChatRequest) -> TalosResult<AttachmentUsage> {
        let mut attachments = Vec::new();
        for message in &request.messages {
            message.validate_attachments()?;
            if let Some(items) = message.attachments.as_ref() {
                attachments.extend(items.iter().cloned());
            }
        }
        let usage = self.validate_attachments(&request.model, &attachments)?;
        let _total_tokens = validate_attachment_usage(attachments.len(), &usage)?;
        Ok(usage)
    }

    /// Native tool-calling request. Default implementation falls back to plain chat.
    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat(request).await
    }

    /// Provider-neutral model block stream.
    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        Err(TalosError::internal(
            "The configured provider must implement stream_model_blocks directly.",
        )
        .context(format!(
            "{} provider must implement stream_model_blocks directly",
            self.name()
        )))
    }
}

/// Resolves the concrete provider that should execute a run.
///
/// Implementations own provider construction, reload, and lookup. Callers pass
/// the effective provider selected by configuration or session state and must
/// not infer providers from model ids.
pub trait LlmProviderRouter: Send + Sync {
    fn provider_for(&self, model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>>;
}

/// Chat request.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub top_p: Option<f32>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub speed: Option<ModelSpeed>,
}

impl ChatRequest {
    pub fn builder() -> ChatRequestBuilder {
        ChatRequestBuilder::default()
    }

    pub fn new_user(message: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            messages: vec![ChatMessage::user(message)],
            model: model.into(),
            temperature: None,
            max_tokens: None,
            stop: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
            speed: None,
        }
    }
}

/// Provider-neutral chat message.
///
/// Construct messages with [`ChatMessage::system`], [`ChatMessage::user`],
/// [`ChatMessage::user_with_attachments`], [`ChatMessage::assistant`], or
/// [`ChatMessage::tool`]. This keeps role-specific invariants explicit and is
/// the supported construction path for downstream crates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// Persisted FileService references for tool images; re-resolved before model calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_file_ids: Vec<String>,
    /// Ordered binary inputs for a user or tool message.
    ///
    /// `None` is the only no-attachment state. Generic message serialization
    /// skips this field; shared protocol encoders are its only wire owners.
    #[serde(skip, default)]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<FunctionCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_continuation: Option<ReasoningContinuation>,
    // Only RequestLevel providers should consume this field. Unsupported providers must drop it
    // during request conversion rather than serializing it into outbound payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: CacheControlKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheControlKind {
    #[serde(rename = "ephemeral")]
    Ephemeral,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
        }
    }

    /// Creates a user message with one or more ordered attachments.
    pub fn user_with_attachments(
        content: impl Into<String>,
        attachments: Vec<Attachment>,
    ) -> TalosResult<Self> {
        if attachments.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "attached user message must include at least one attachment".to_string(),
            ));
        }
        Ok(Self {
            attachments: Some(attachments),
            image_file_ids: Vec::new(),
            ..Self::user(content)
        })
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<FunctionCall>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(tool_calls),
            reasoning_continuation: None,
            cache_control: None,
        }
    }

    pub fn tool(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            attachments: None,
            image_file_ids: Vec::new(),
            name: Some(name.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
            reasoning_continuation: None,
            cache_control: None,
        }
    }

    /// Attaches provider continuation required to replay this assistant message.
    pub fn with_reasoning_continuation(mut self, continuation: ReasoningContinuation) -> Self {
        self.reasoning_continuation = Some(continuation);
        self
    }
}

/// Message role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Provider-neutral reasoning output split into display and replay projections.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningOutput {
    #[serde(default)]
    pub display_text: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ReasoningContinuation>,
}

impl ReasoningOutput {
    pub fn display_text(&self) -> String {
        self.display_text.join("")
    }
}

/// Provider continuation replayed only inside the model conversation state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ReasoningContinuation {
    Text(String),
    Blocks(Vec<ReasoningBlock>),
}

/// Ordered provider reasoning block retained for tool-loop replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ReasoningBlock {
    Text(ReasoningTextBlock),
    Summary(ReasoningSummaryBlock),
    Encrypted(ReasoningEncryptedBlock),
}

/// Provider reasoning text retained with its replay identity and signature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningTextBlock {
    pub provider_item_id: Option<String>,
    pub provider_index: Option<u32>,
    pub text: String,
    pub signature: Option<String>,
    pub format: Option<String>,
}

/// Provider reasoning summary retained in its original block position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningSummaryBlock {
    pub provider_item_id: Option<String>,
    pub provider_index: Option<u32>,
    pub text: String,
    pub format: Option<String>,
}

/// Opaque provider reasoning payload retained only for continuation replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReasoningEncryptedBlock {
    pub provider_item_id: Option<String>,
    pub provider_index: Option<u32>,
    pub data: String,
    pub format: Option<String>,
}

/// Chat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    #[serde(default)]
    pub reasoning: ReasoningOutput,
    pub model: String,
    pub usage: Usage,
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<FunctionCall>>,
}

impl ChatResponse {
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .as_ref()
            .map(|calls| !calls.is_empty())
            .unwrap_or(false)
    }

    pub fn get_tool_calls(&self) -> &[FunctionCall] {
        self.tool_calls.as_deref().unwrap_or(&[])
    }
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

/// Streaming chunk for plain text APIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChunk {
    pub delta: String,
    pub finish_reason: Option<String>,
}

/// Plain text streaming response.
pub struct ChatStream {
    pub rx: mpsc::Receiver<TalosResult<ChatChunk>>,
}

impl ChatStream {
    pub fn new(rx: mpsc::Receiver<TalosResult<ChatChunk>>) -> Self {
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<TalosResult<ChatChunk>> {
        self.rx.recv().await
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelBlockKind {
    Reasoning,
    AssistantText,
    ToolCall,
    ToolResult,
    SystemNotice,
}

/// Provider-reported phase for an assistant text model output block.
///
/// This is the model protocol boundary value. Agent kernel code must normalize
/// it into its own assistant text phase before emitting kernel stream events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ModelOutputPhase {
    #[default]
    Unknown,
    Commentary,
    FinalAnswer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelToolCallStart {
    pub index: usize,
    pub call_id: Option<String>,
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelBlockStarted {
    pub model_item_id: String,
    pub provider_item_id: Option<String>,
    pub kind: ModelBlockKind,
    #[serde(default)]
    pub output_phase: ModelOutputPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ModelToolCallStart>,
}

impl ModelBlockStarted {
    pub fn reasoning(provider_item_id: Option<String>) -> Self {
        Self {
            model_item_id: "reasoning".to_string(),
            provider_item_id,
            kind: ModelBlockKind::Reasoning,
            output_phase: ModelOutputPhase::Unknown,
            tool_call: None,
        }
    }

    pub fn assistant_text(
        provider_item_id: Option<String>,
        output_phase: ModelOutputPhase,
    ) -> Self {
        Self {
            model_item_id: "assistant_text".to_string(),
            provider_item_id,
            kind: ModelBlockKind::AssistantText,
            output_phase,
            tool_call: None,
        }
    }

    pub fn tool_call(index: usize, call_id: Option<String>, tool_name: Option<String>) -> Self {
        let model_item_id = format!("tool_call_{index}");
        Self {
            model_item_id,
            provider_item_id: call_id.clone(),
            kind: ModelBlockKind::ToolCall,
            output_phase: ModelOutputPhase::Unknown,
            tool_call: Some(ModelToolCallStart {
                index,
                call_id,
                tool_name,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelBlockDelta {
    ReasoningText { delta: String },
    AssistantText { delta: String },
    ToolArguments { delta: String },
    ToolProgress { delta: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelBlockCompleted {
    pub model_item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelBlockStreamEvent {
    BlockStarted(ModelBlockStarted),
    BlockDelta {
        model_item_id: String,
        delta: ModelBlockDelta,
    },
    BlockCompleted(ModelBlockCompleted),
    ReasoningCompleted {
        output: ReasoningOutput,
    },
    Done {
        finish_reason: Option<String>,
        #[serde(default)]
        usage: Option<Usage>,
    },
}

pub struct ModelBlockStream {
    pub rx: mpsc::Receiver<TalosResult<ModelBlockStreamEvent>>,
}

impl ModelBlockStream {
    pub fn new(rx: mpsc::Receiver<TalosResult<ModelBlockStreamEvent>>) -> Self {
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<TalosResult<ModelBlockStreamEvent>> {
        self.rx.recv().await
    }
}

/// Incrementally decodes complete SSE lines from arbitrary byte chunks.
#[derive(Debug, Default)]
pub struct SseLineDecoder {
    buffer: Vec<u8>,
}

impl SseLineDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<TalosResult<String>> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line_bytes: Vec<u8> = self.buffer.drain(..=pos).collect();
            if line_bytes.last() == Some(&b'\n') {
                line_bytes.pop();
            }
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }
            lines.push(String::from_utf8(line_bytes).map_err(|err| {
                TalosError::internal_with_source(
                    "SSE line is not valid UTF-8; technical details were withheld.",
                    err,
                )
            }));
        }
        lines
    }

    pub fn has_pending_fragment(&self) -> bool {
        !self.buffer.is_empty()
    }
}

/// Retry settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u64,
    #[serde(default = "default_base_delay")]
    pub base_delay_ms: u64,
    #[serde(default = "default_true")]
    pub retry_429: bool,
    #[serde(default = "default_true")]
    pub retry_5xx: bool,
}

fn default_max_attempts() -> u64 {
    3
}
fn default_base_delay() -> u64 {
    1000
}
fn default_true() -> bool {
    true
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            base_delay_ms: default_base_delay(),
            retry_429: true,
            retry_5xx: true,
        }
    }
}

/// Runtime transport configuration for one Provider client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRuntimeConfig {
    pub name: String,
    pub base_url: String,
    #[serde(skip_serializing)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    60
}

impl ProviderRuntimeConfig {
    pub fn anthropic(api_key: impl Into<String>) -> Self {
        Self {
            name: "anthropic".into(),
            base_url: "https://api.anthropic.com/v1".into(),
            api_key: Some(api_key.into()),
            retry: RetryConfig::default(),
            timeout_secs: 60,
        }
    }

    pub fn openai(api_key: impl Into<String>) -> Self {
        Self {
            name: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            api_key: Some(api_key.into()),
            retry: RetryConfig::default(),
            timeout_secs: 60,
        }
    }

    pub fn ollama(base_url: impl Into<String>) -> Self {
        Self {
            name: "ollama".into(),
            base_url: base_url.into(),
            api_key: None,
            retry: RetryConfig::default(),
            timeout_secs: 120,
        }
    }
}

/// Request builder.
#[derive(Default)]
pub struct ChatRequestBuilder {
    messages: Option<Vec<ChatMessage>>,
    model: Option<String>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    stop: Option<Vec<String>>,
    top_p: Option<f32>,
    tools: Option<Vec<ToolDefinition>>,
    tool_choice: Option<ToolChoice>,
    parallel_tool_calls: Option<bool>,
    reasoning_effort: Option<ReasoningEffort>,
    speed: Option<ModelSpeed>,
}

impl ChatRequestBuilder {
    pub fn messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = Some(messages);
        self
    }

    pub fn message(mut self, content: impl Into<String>) -> Self {
        let messages = self.messages.get_or_insert_with(Vec::new);
        messages.push(ChatMessage::user(content));
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }

    pub fn top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    pub fn parallel_tool_calls(mut self, enabled: bool) -> Self {
        self.parallel_tool_calls = Some(enabled);
        self
    }

    pub fn reasoning_effort(mut self, reasoning_effort: Option<ReasoningEffort>) -> Self {
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn speed(mut self, speed: ModelSpeed) -> Self {
        self.speed = Some(speed);
        self
    }

    pub fn build(self) -> ChatRequest {
        ChatRequest {
            messages: self.messages.unwrap_or_default(),
            model: self.model.expect("model is required"),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            stop: self.stop,
            top_p: self.top_p,
            tools: self.tools,
            tool_choice: self.tool_choice,
            parallel_tool_calls: self.parallel_tool_calls,
            reasoning_effort: self.reasoning_effort,
            speed: self.speed,
        }
    }
}
