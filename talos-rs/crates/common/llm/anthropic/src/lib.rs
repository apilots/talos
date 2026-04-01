//! Talos LLM Anthropic provider.

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider,
    NativeThinkingConfig, NativeThinkingSupport, Role, ToolChoice, ToolDefinition, ToolStream,
    ToolStreamEvent, Usage,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Anthropic API configuration.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            timeout_secs: 60,
        }
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: Value,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    #[serde(rename = "input_schema")]
    input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<AnthropicToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<AnthropicThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct AnthropicThinkingConfig {
    r#type: String,
    budget_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    #[serde(rename = "type")]
    response_type: String,
    #[allow(dead_code)]
    role: String,
    content: Vec<AnthropicContent>,
    model: String,
    stop_reason: Option<String>,
    usage: AnthropicUsage,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Anthropic LLM provider.
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
    }

    fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            header::HeaderValue::from_str(&self.config.api_key).unwrap(),
        );
        headers.insert(
            "anthropic-version",
            header::HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    fn convert_tool_choice(choice: Option<ToolChoice>) -> Option<AnthropicToolChoice> {
        match choice {
            Some(ToolChoice::Auto) => Some(AnthropicToolChoice::Auto),
            Some(ToolChoice::Required) => Some(AnthropicToolChoice::Any),
            Some(ToolChoice::Named { name }) => Some(AnthropicToolChoice::Tool { name }),
            Some(ToolChoice::None) | None => None,
        }
    }

    fn convert_tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<AnthropicTool>> {
        tools.map(|defs| {
            defs.into_iter()
                .map(|tool| AnthropicTool {
                    name: tool.name,
                    description: tool.description,
                    input_schema: tool
                        .parameters
                        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
                })
                .collect()
        })
    }

    fn convert_messages(
        messages: Vec<ChatMessage>,
        override_system: Option<String>,
    ) -> (Option<String>, Vec<AnthropicMessage>) {
        let mut system_prompt = override_system;
        let mut anthropic_messages = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    if system_prompt.is_none() {
                        system_prompt = Some(msg.content);
                    }
                }
                Role::User => {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: Value::String(msg.content),
                    });
                }
                Role::Assistant => {
                    if let Some(tool_calls) = msg.tool_calls {
                        let mut blocks = Vec::new();
                        if !msg.content.is_empty() {
                            blocks.push(serde_json::json!({
                                "type": "text",
                                "text": msg.content,
                            }));
                        }
                        for call in tool_calls {
                            blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": call.id,
                                "name": call.name,
                                "input": call.arguments,
                            }));
                        }
                        anthropic_messages.push(AnthropicMessage {
                            role: "assistant".to_string(),
                            content: Value::Array(blocks),
                        });
                    } else {
                        anthropic_messages.push(AnthropicMessage {
                            role: "assistant".to_string(),
                            content: Value::String(msg.content),
                        });
                    }
                }
                Role::Tool => {
                    let tool_use_id = msg.tool_call_id.unwrap_or_default();
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: Value::Array(vec![serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": msg.content,
                            "is_error": false,
                        })]),
                    });
                }
            }
        }

        (system_prompt, anthropic_messages)
    }

    fn parse_response_content(content: &[AnthropicContent]) -> (String, Option<Vec<FunctionCall>>) {
        let mut text = Vec::new();
        let mut calls = Vec::new();

        for block in content {
            match block.content_type.as_str() {
                "text" => {
                    if let Some(fragment) = block.text.as_ref() {
                        text.push(fragment.clone());
                    }
                }
                "tool_use" => {
                    calls.push(FunctionCall::new(
                        block.id.clone().unwrap_or_default(),
                        block.name.clone().unwrap_or_default(),
                        block.input.clone().unwrap_or(Value::Null),
                    ));
                }
                _ => {}
            }
        }

        let tool_calls = if calls.is_empty() { None } else { Some(calls) };
        (text.join(""), tool_calls)
    }

    fn build_request(request: ChatRequest, stream: bool) -> AnthropicRequest {
        let (system, messages) = Self::convert_messages(request.messages, request.system);
        AnthropicRequest {
            model: request.model,
            messages,
            system,
            max_tokens: request.max_tokens.unwrap_or(1024),
            temperature: request.temperature,
            top_p: request.top_p,
            stop_sequences: request.stop,
            tools: Self::convert_tools(request.tools),
            tool_choice: Self::convert_tool_choice(request.tool_choice),
            thinking: request
                .native_thinking
                .as_ref()
                .map(Self::convert_native_thinking),
            stream: Some(stream),
        }
    }

    fn convert_native_thinking(config: &NativeThinkingConfig) -> AnthropicThinkingConfig {
        AnthropicThinkingConfig {
            r#type: "enabled".to_string(),
            budget_tokens: config.budget_tokens.unwrap_or(2048),
        }
    }

    fn supports_native_thinking_model(model: &str) -> bool {
        let normalized = model.trim().to_ascii_lowercase();
        normalized.starts_with("claude-3-7-sonnet")
            || normalized.starts_with("claude-sonnet-4")
            || normalized.starts_with("claude-opus-4")
            || normalized.starts_with("claude-4")
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let anthropic_req = Self::build_request(request, false);
        let url = format!("{}/messages", self.config.base_url);
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", anthropic_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&anthropic_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("Anthropic API error: {} - {}", status, error_text);
            return Err(TalosError::InternalError(format!(
                "Anthropic API error: {} - {}",
                status, error_text
            )));
        }

        let anthropic_resp: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        let (content, tool_calls) = Self::parse_response_content(&anthropic_resp.content);

        Ok(ChatResponse {
            content,
            model: anthropic_resp.model,
            usage: Usage {
                prompt_tokens: anthropic_resp.usage.input_tokens,
                completion_tokens: anthropic_resp.usage.output_tokens,
                total_tokens: anthropic_resp.usage.input_tokens
                    + anthropic_resp.usage.output_tokens,
            },
            finish_reason: anthropic_resp.stop_reason,
            tool_calls,
        })
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let anthropic_req = Self::build_request(request, true);

        let url = format!("{}/messages", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&anthropic_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(TalosError::InternalError(format!(
                "Anthropic API error: {} - {}",
                status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn default_model(&self) -> &str {
        "claude-3-5-sonnet-20241022"
    }

    fn native_thinking_support(&self, model: &str) -> NativeThinkingSupport {
        if Self::supports_native_thinking_model(model) {
            NativeThinkingSupport::Supported
        } else {
            NativeThinkingSupport::Unsupported
        }
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to Anthropic API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to Anthropic API");
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if !line.starts_with("data: ") {
                                continue;
                            }
                            let data = &line[6..];
                            let Ok(event) = serde_json::from_str::<Value>(data) else {
                                continue;
                            };

                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match event_type {
                                "content_block_delta" => {
                                    if let Some(delta) = event
                                        .get("delta")
                                        .and_then(|d| d.get("text"))
                                        .and_then(|t| t.as_str())
                                    {
                                        let _ = tx
                                            .send(Ok(ChatChunk {
                                                delta: delta.to_string(),
                                                finish_reason: None,
                                            }))
                                            .await;
                                    }
                                }
                                "message_stop" => {
                                    let finish_reason = event
                                        .get("stop_reason")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("stop")
                                        .to_string();
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: String::new(),
                                            finish_reason: Some(finish_reason),
                                        }))
                                        .await;
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(TalosError::InternalError(format!(
                                "Stream error: {}",
                                e
                            ))))
                            .await;
                        return;
                    }
                }
            }
        });

        Ok(ChatStream::new(rx))
    }

    async fn stream_with_tools(&self, request: ChatRequest) -> TalosResult<ToolStream> {
        let response = self.send_stream(request).await?;
        let (tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(100);

        tokio::spawn(async move {
            #[derive(Default)]
            struct ToolAcc {
                id: Option<String>,
                name: Option<String>,
                arguments: String,
                started: bool,
            }

            let mut stream = response.bytes_stream();
            let mut tool_acc: HashMap<usize, ToolAcc> = HashMap::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if !line.starts_with("data: ") {
                                continue;
                            }
                            let data = &line[6..];
                            let Ok(event) = serde_json::from_str::<Value>(data) else {
                                continue;
                            };

                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match event_type {
                                "content_block_start" => {
                                    let index =
                                        event.get("index").and_then(|v| v.as_u64()).unwrap_or(0)
                                            as usize;
                                    let block = event.get("content_block");

                                    if block.and_then(|b| b.get("type")).and_then(|v| v.as_str())
                                        == Some("tool_use")
                                    {
                                        let entry = tool_acc.entry(index).or_default();
                                        entry.id = block
                                            .and_then(|b| b.get("id"))
                                            .and_then(|v| v.as_str())
                                            .map(|v| v.to_string());
                                        entry.name = block
                                            .and_then(|b| b.get("name"))
                                            .and_then(|v| v.as_str())
                                            .map(|v| v.to_string());
                                        if !entry.started {
                                            entry.started = true;
                                            if tx
                                                .send(Ok(ToolStreamEvent::ToolCallStart {
                                                    index,
                                                    id: entry.id.clone(),
                                                    name: entry.name.clone(),
                                                }))
                                                .await
                                                .is_err()
                                            {
                                                return;
                                            }
                                        }
                                    }
                                }
                                "content_block_delta" => {
                                    let index =
                                        event.get("index").and_then(|v| v.as_u64()).unwrap_or(0)
                                            as usize;

                                    if let Some(reasoning_delta) = event
                                        .get("delta")
                                        .and_then(|d| d.get("thinking"))
                                        .and_then(|v| v.as_str())
                                        .or_else(|| {
                                            event
                                                .get("delta")
                                                .and_then(|d| d.get("reasoning"))
                                                .and_then(|v| v.as_str())
                                        })
                                    {
                                        if tx
                                            .send(Ok(ToolStreamEvent::ReasoningDelta {
                                                delta: reasoning_delta.to_string(),
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }

                                    if let Some(text_delta) = event
                                        .get("delta")
                                        .and_then(|d| d.get("text"))
                                        .and_then(|v| v.as_str())
                                    {
                                        if tx
                                            .send(Ok(ToolStreamEvent::TextDelta {
                                                delta: text_delta.to_string(),
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }

                                    if let Some(partial_json) = event
                                        .get("delta")
                                        .and_then(|d| d.get("partial_json"))
                                        .and_then(|v| v.as_str())
                                    {
                                        let entry = tool_acc.entry(index).or_default();
                                        entry.arguments.push_str(partial_json);
                                        if tx
                                            .send(Ok(ToolStreamEvent::ToolCallDelta {
                                                index,
                                                arguments_delta: partial_json.to_string(),
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                "content_block_stop" => {
                                    let index =
                                        event.get("index").and_then(|v| v.as_u64()).unwrap_or(0)
                                            as usize;
                                    if let Some(acc) = tool_acc.remove(&index) {
                                        let arguments =
                                            serde_json::from_str::<Value>(&acc.arguments)
                                                .unwrap_or_else(|_| Value::String(acc.arguments));
                                        if tx
                                            .send(Ok(ToolStreamEvent::ToolCallEnd {
                                                index,
                                                id: acc.id,
                                                name: acc.name,
                                                arguments,
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                                "message_stop" => {
                                    let finish_reason = event
                                        .get("stop_reason")
                                        .and_then(|v| v.as_str())
                                        .map(|v| v.to_string())
                                        .or_else(|| Some("stop".to_string()));
                                    let _ = tx
                                        .send(Ok(ToolStreamEvent::Done {
                                            finish_reason,
                                            usage: None,
                                        }))
                                        .await;
                                    return;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(TalosError::InternalError(format!(
                                "Stream error: {}",
                                e
                            ))))
                            .await;
                        return;
                    }
                }
            }
        });

        Ok(ToolStream::new(rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_llm_provider::NativeThinkingEffort;

    #[test]
    fn test_anthropic_config_builder() {
        let config = AnthropicConfig::new("test-api-key")
            .with_base_url("https://custom.anthropic.com/v1")
            .with_timeout(120);

        assert_eq!(config.api_key, "test-api-key");
        assert_eq!(config.base_url, "https://custom.anthropic.com/v1");
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are a helpful assistant"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];

        let (system, anthropic_msgs) = AnthropicProvider::convert_messages(messages, None);

        assert_eq!(system, Some("You are a helpful assistant".to_string()));
        assert_eq!(anthropic_msgs.len(), 2);
        assert_eq!(anthropic_msgs[0].role, "user");
        assert_eq!(
            anthropic_msgs[0].content,
            Value::String("Hello".to_string())
        );
        assert_eq!(anthropic_msgs[1].role, "assistant");
        assert_eq!(
            anthropic_msgs[1].content,
            Value::String("Hi there!".to_string())
        );
    }

    #[test]
    fn test_convert_messages_with_tool_result() {
        let messages = vec![ChatMessage::tool("call_1", "read", "done")];
        let (_system, anthropic_msgs) = AnthropicProvider::convert_messages(messages, None);
        assert_eq!(anthropic_msgs.len(), 1);
        assert_eq!(anthropic_msgs[0].role, "user");
        assert!(anthropic_msgs[0].content.is_array());
    }

    #[test]
    fn test_provider_name() {
        let config = AnthropicConfig::new("test-key");
        let provider = AnthropicProvider::new(config);
        assert_eq!(provider.name(), "anthropic");
        assert_eq!(provider.default_model(), "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn test_native_thinking_support_is_model_gated() {
        let provider = AnthropicProvider::new(AnthropicConfig::new("test-key"));

        assert_eq!(
            provider.native_thinking_support("claude-3-5-sonnet-20241022"),
            NativeThinkingSupport::Unsupported
        );
        assert_eq!(
            provider.native_thinking_support("claude-3-7-sonnet-20250219"),
            NativeThinkingSupport::Supported
        );
    }

    #[test]
    fn test_build_headers() {
        let config = AnthropicConfig::new("test-api-key");
        let provider = AnthropicProvider::new(config);
        let headers = provider.build_headers();

        assert!(headers.contains_key("x-api-key"));
        assert!(headers.contains_key("anthropic-version"));
        assert!(headers.contains_key("content-type"));
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![ToolDefinition::new(
            "search",
            "Search docs",
            Some(serde_json::json!({"type": "object"})),
        )];
        let converted = AnthropicProvider::convert_tools(Some(tools)).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name, "search");
    }

    #[test]
    fn test_build_request_maps_native_thinking_budget() {
        let request = ChatRequest::builder()
            .model("claude-3-5-sonnet-20241022")
            .messages(vec![ChatMessage::user("hello")])
            .native_thinking(NativeThinkingConfig {
                effort: NativeThinkingEffort::Medium,
                budget_tokens: Some(4096),
            })
            .build();

        let built = AnthropicProvider::build_request(request, false);

        assert_eq!(
            built
                .thinking
                .as_ref()
                .map(|thinking| thinking.budget_tokens),
            Some(4096)
        );
    }
}
