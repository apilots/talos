//! Talos LLM provider abstraction.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use talos_error::TalosResult;
use thiserror::Error;
use tokio::sync::mpsc;

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

/// LLM provider trait.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse>;
    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream>;
    fn default_model(&self) -> &str;
    fn native_thinking_support(&self, _model: &str) -> NativeThinkingSupport {
        NativeThinkingSupport::Unsupported
    }

    /// Native tool-calling request. Default implementation falls back to plain chat.
    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat(request).await
    }

    /// Tool-aware stream. Default implementation wraps text chunks into text events.
    async fn stream_with_tools(&self, request: ChatRequest) -> TalosResult<ToolStream> {
        let mut stream = self.stream(request).await?;
        let (tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(100);

        tokio::spawn(async move {
            while let Some(result) = stream.next().await {
                match result {
                    Ok(chunk) => {
                        if !chunk.delta.is_empty() {
                            if tx
                                .send(Ok(ToolStreamEvent::TextDelta { delta: chunk.delta }))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }

                        if chunk.finish_reason.is_some() {
                            let _ = tx
                                .send(Ok(ToolStreamEvent::Done {
                                    finish_reason: chunk.finish_reason,
                                    usage: None,
                                }))
                                .await;
                            return;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err)).await;
                        return;
                    }
                }
            }
        });

        Ok(ToolStream::new(rx))
    }
}

/// Chat request.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub system: Option<String>,
    pub stop: Option<Vec<String>>,
    pub top_p: Option<f32>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
    pub native_thinking: Option<NativeThinkingConfig>,
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
            system: None,
            stop: None,
            top_p: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            native_thinking: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeThinkingEffort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NativeThinkingConfig {
    pub effort: NativeThinkingEffort,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NativeThinkingSupport {
    #[default]
    Unsupported,
    Supported,
}

impl NativeThinkingSupport {
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// Chat message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<FunctionCall>>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<FunctionCall>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            name: None,
            tool_call_id: None,
            tool_calls: Some(tool_calls),
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
            name: Some(name.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
        }
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

/// Chat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
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

/// Tool-aware stream event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolStreamEvent {
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    ToolCallStart {
        index: usize,
        id: Option<String>,
        name: Option<String>,
    },
    ToolCallDelta {
        index: usize,
        arguments_delta: String,
    },
    ToolCallEnd {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments: Value,
    },
    Done {
        finish_reason: Option<String>,
        #[serde(default)]
        usage: Option<Usage>,
    },
}

/// Tool-aware streaming response.
pub struct ToolStream {
    pub rx: mpsc::Receiver<TalosResult<ToolStreamEvent>>,
}

impl ToolStream {
    pub fn new(rx: mpsc::Receiver<TalosResult<ToolStreamEvent>>) -> Self {
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<TalosResult<ToolStreamEvent>> {
        self.rx.recv().await
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

/// Provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
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

impl ProviderConfig {
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
    system: Option<String>,
    stop: Option<Vec<String>>,
    top_p: Option<f32>,
    tools: Option<Vec<ToolDefinition>>,
    tool_choice: Option<ToolChoice>,
    parallel_tool_calls: Option<bool>,
    native_thinking: Option<NativeThinkingConfig>,
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

    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
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

    pub fn native_thinking(mut self, config: NativeThinkingConfig) -> Self {
        self.native_thinking = Some(config);
        self
    }

    pub fn build(self) -> ChatRequest {
        ChatRequest {
            messages: self.messages.unwrap_or_default(),
            model: self.model.expect("model is required"),
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            system: self.system,
            stop: self.stop,
            top_p: self.top_p,
            tools: self.tools,
            tool_choice: self.tool_choice,
            parallel_tool_calls: self.parallel_tool_calls,
            native_thinking: self.native_thinking,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message() {
        let msg = ChatMessage::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "hello");
    }

    #[test]
    fn test_chat_request_builder() {
        let req = ChatRequest::builder()
            .message("hello")
            .model("claude-3-5-sonnet")
            .temperature(0.7)
            .build();

        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.model, "claude-3-5-sonnet");
        assert_eq!(req.temperature, Some(0.7));
    }

    #[test]
    fn test_provider_config() {
        let config = ProviderConfig::anthropic("test-key");
        assert_eq!(config.name, "anthropic");
        assert!(config.base_url.contains("anthropic.com"));
    }

    #[test]
    fn test_function_call_roundtrip() {
        let call = FunctionCall::new("call_1", "read", serde_json::json!({"path": "/tmp/a"}));
        let encoded = serde_json::to_string(&call).unwrap();
        let decoded: FunctionCall = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, call);
    }

    #[test]
    fn test_function_response_error() {
        let response = FunctionResponse::error("call_1", "read", serde_json::json!("failed"));
        assert!(response.is_error);
    }

    #[test]
    fn test_tool_definition_validation() {
        let def = ToolDefinition::new(
            "read",
            "Read file",
            Some(serde_json::json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": {"type": "string"}
                }
            })),
        );

        assert!(def
            .validate_arguments(&serde_json::json!({"path": "/tmp/a"}))
            .is_ok());
        assert!(def.validate_arguments(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_chat_response_tool_calls_helper() {
        let response = ChatResponse {
            content: String::new(),
            model: "mock".to_string(),
            usage: Usage::default(),
            finish_reason: Some("tool_calls".to_string()),
            tool_calls: Some(vec![FunctionCall::new("1", "read", serde_json::json!({}))]),
        };

        assert!(response.has_tool_calls());
        assert_eq!(response.get_tool_calls().len(), 1);
    }
}
