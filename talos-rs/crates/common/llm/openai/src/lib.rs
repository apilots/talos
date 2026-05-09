//! Talos LLM OpenAI provider.

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
    NativeThinkingConfig, NativeThinkingEffort, NativeThinkingSupport, Role, ToolChoice,
    ToolDefinition, ToolStream, ToolStreamEvent, Usage,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// OpenAI API configuration.
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
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
struct OpenAIMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAITool {
    r#type: String,
    function: OpenAIFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIFunctionDefinition {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIToolCall {
    id: String,
    r#type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OpenAIToolChoice {
    Simple(String),
    Named {
        r#type: String,
        function: OpenAIToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIToolChoiceFunction {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<OpenAIToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAIStreamOptions>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    object: String,
    #[allow(dead_code)]
    created: u64,
    model: String,
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIChoice {
    #[allow(dead_code)]
    index: u32,
    message: OpenAIResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// OpenAI LLM provider.
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: Client,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
    }

    fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key)).unwrap(),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    fn convert_tool_choice(choice: Option<ToolChoice>) -> Option<OpenAIToolChoice> {
        match choice {
            Some(ToolChoice::Auto) => Some(OpenAIToolChoice::Simple("auto".to_string())),
            Some(ToolChoice::Required) => Some(OpenAIToolChoice::Simple("required".to_string())),
            Some(ToolChoice::None) => Some(OpenAIToolChoice::Simple("none".to_string())),
            Some(ToolChoice::Named { name }) => Some(OpenAIToolChoice::Named {
                r#type: "function".to_string(),
                function: OpenAIToolChoiceFunction { name },
            }),
            None => None,
        }
    }

    fn convert_tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<OpenAITool>> {
        tools.map(|definitions| {
            definitions
                .into_iter()
                .map(|tool| OpenAITool {
                    r#type: "function".to_string(),
                    function: OpenAIFunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        })
    }

    fn convert_messages(messages: Vec<ChatMessage>) -> Vec<OpenAIMessage> {
        messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                }
                .to_string();

                let tool_calls = msg.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|call| OpenAIToolCall {
                            id: call.id,
                            r#type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        })
                        .collect()
                });

                OpenAIMessage {
                    role,
                    content: Some(msg.content),
                    name: msg.name,
                    tool_call_id: msg.tool_call_id,
                    tool_calls,
                }
            })
            .collect()
    }

    fn parse_tool_calls(tool_calls: Option<Vec<OpenAIToolCall>>) -> Option<Vec<FunctionCall>> {
        tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|call| {
                    let parsed = serde_json::from_str::<Value>(&call.function.arguments)
                        .unwrap_or_else(|_| Value::String(call.function.arguments.clone()));
                    FunctionCall::new(call.id, call.function.name, parsed)
                })
                .collect()
        })
    }

    fn extract_reasoning_delta(delta: &Value) -> Option<String> {
        for key in ["reasoning"] {
            if let Some(value) = delta.get(key) {
                match value {
                    Value::String(text) if !text.is_empty() => return Some(text.clone()),
                    Value::Array(items) => {
                        let mut merged = String::new();
                        for item in items {
                            if let Some(text) = item
                                .get("text")
                                .and_then(|v| v.as_str())
                                .or_else(|| item.get("content").and_then(|v| v.as_str()))
                                .or_else(|| item.as_str())
                            {
                                merged.push_str(text);
                            }
                        }
                        if !merged.is_empty() {
                            return Some(merged);
                        }
                    }
                    Value::Object(map) => {
                        if let Some(text) = map
                            .get("text")
                            .and_then(|v| v.as_str())
                            .or_else(|| map.get("content").and_then(|v| v.as_str()))
                        {
                            if !text.is_empty() {
                                return Some(text.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn build_request(request: ChatRequest, stream: bool) -> OpenAIRequest {
        OpenAIRequest {
            model: request.model,
            messages: Self::convert_messages(request.messages),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stop: request.stop,
            tools: Self::convert_tools(request.tools),
            tool_choice: Self::convert_tool_choice(request.tool_choice),
            parallel_tool_calls: request.parallel_tool_calls,
            reasoning_effort: request
                .native_thinking
                .as_ref()
                .map(Self::convert_native_thinking_effort),
            stream: Some(stream),
            stream_options: if stream {
                Some(OpenAIStreamOptions {
                    include_usage: true,
                })
            } else {
                None
            },
        }
    }

    fn convert_native_thinking_effort(config: &NativeThinkingConfig) -> String {
        match config.effort {
            NativeThinkingEffort::Low => "low",
            NativeThinkingEffort::Medium => "medium",
            NativeThinkingEffort::High => "high",
        }
        .to_string()
    }

    fn supports_native_thinking_model(model: &str) -> bool {
        let normalized = model.trim().to_ascii_lowercase();
        normalized.starts_with("o1")
            || normalized.starts_with("o3")
            || normalized.starts_with("o4")
            || normalized.starts_with("gpt-5")
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let openai_req = Self::build_request(request, false);

        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", openai_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&openai_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("OpenAI API error: {} - {}", status, error_text);
            return Err(TalosError::InternalError(format!(
                "OpenAI API error: {} - {}",
                status, error_text
            )));
        }

        let openai_resp: OpenAIResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        let choice = openai_resp.choices.into_iter().next().ok_or_else(|| {
            TalosError::InternalError("No choices in OpenAI response".to_string())
        })?;

        let usage = openai_resp.usage.unwrap_or(OpenAIUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        });

        Ok(ChatResponse {
            content: choice.message.content.unwrap_or_default(),
            reasoning: Vec::new(),
            model: openai_resp.model,
            usage: Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            finish_reason: choice.finish_reason,
            tool_calls: Self::parse_tool_calls(choice.message.tool_calls),
        })
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let openai_req = Self::build_request(request, true);
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&openai_req)
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
                "OpenAI API error: {} - {}",
                status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn default_model(&self) -> String {
        "gpt-4o".to_string()
    }

    fn native_thinking_support(&self, model: &str) -> NativeThinkingSupport {
        if Self::supports_native_thinking_model(model) {
            NativeThinkingSupport::Supported
        } else {
            NativeThinkingSupport::Unsupported
        }
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to OpenAI API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to OpenAI API");
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut final_finish_reason: Option<String> = None;
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if !line.starts_with("data: ") {
                                continue;
                            }

                            let data = &line[6..];
                            if data == "[DONE]" {
                                let _ = tx
                                    .send(Ok(ChatChunk {
                                        delta: String::new(),
                                        finish_reason: final_finish_reason
                                            .clone()
                                            .or_else(|| Some("stop".to_string())),
                                    }))
                                    .await;
                                return;
                            }

                            if let Ok(event) = serde_json::from_str::<Value>(data) {
                                let choice = event
                                    .get("choices")
                                    .and_then(|c| c.as_array())
                                    .and_then(|arr| arr.first());

                                if let Some(delta) = choice
                                    .and_then(|c| c.get("delta"))
                                    .and_then(|d| d.get("content"))
                                    .and_then(|t| t.as_str())
                                {
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: delta.to_string(),
                                            finish_reason: None,
                                        }))
                                        .await;
                                }

                                if let Some(finish_reason) = choice
                                    .and_then(|c| c.get("finish_reason"))
                                    .and_then(|r| r.as_str())
                                {
                                    final_finish_reason = Some(finish_reason.to_string());
                                }
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
            struct Acc {
                id: Option<String>,
                name: Option<String>,
                arguments: String,
                started: bool,
            }

            let mut stream = response.bytes_stream();
            let mut accumulators: HashMap<usize, Acc> = HashMap::new();
            let mut final_finish_reason: Option<String> = None;
            let mut final_usage: Option<Usage> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if !line.starts_with("data: ") {
                                continue;
                            }
                            let data = &line[6..];

                            if data == "[DONE]" {
                                let _ = tx
                                    .send(Ok(ToolStreamEvent::Done {
                                        finish_reason: final_finish_reason
                                            .clone()
                                            .or_else(|| Some("stop".to_string())),
                                        usage: final_usage.clone(),
                                    }))
                                    .await;
                                return;
                            }

                            let Ok(event) = serde_json::from_str::<Value>(data) else {
                                continue;
                            };

                            if let Some(usage) = event.get("usage").and_then(|value| {
                                serde_json::from_value::<OpenAIUsage>(value.clone()).ok()
                            }) {
                                final_usage = Some(Usage {
                                    prompt_tokens: usage.prompt_tokens,
                                    completion_tokens: usage.completion_tokens,
                                    total_tokens: usage.total_tokens,
                                    cache_creation_input_tokens: None,
                                    cache_read_input_tokens: None,
                                });
                            }

                            let choice = event
                                .get("choices")
                                .and_then(|c| c.as_array())
                                .and_then(|arr| arr.first());

                            let delta_object = choice.and_then(|c| c.get("delta"));
                            if let Some(reasoning_delta) =
                                delta_object.and_then(Self::extract_reasoning_delta)
                            {
                                if tx
                                    .send(Ok(ToolStreamEvent::ReasoningDelta {
                                        delta: reasoning_delta,
                                    }))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }

                            if let Some(delta) = delta_object
                                .and_then(|d| d.get("content"))
                                .and_then(|t| t.as_str())
                            {
                                if tx
                                    .send(Ok(ToolStreamEvent::TextDelta {
                                        delta: delta.to_string(),
                                    }))
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }

                            if let Some(tool_calls) = choice
                                .and_then(|c| c.get("delta"))
                                .and_then(|d| d.get("tool_calls"))
                                .and_then(|t| t.as_array())
                            {
                                for tool_call_delta in tool_calls {
                                    let index = tool_call_delta
                                        .get("index")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0)
                                        as usize;
                                    let entry = accumulators.entry(index).or_default();

                                    if let Some(id) = tool_call_delta
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                    {
                                        entry.id = Some(id);
                                    }

                                    if let Some(name) = tool_call_delta
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string())
                                    {
                                        entry.name = Some(name);
                                    }

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

                                    if let Some(arguments_delta) = tool_call_delta
                                        .get("function")
                                        .and_then(|f| f.get("arguments"))
                                        .and_then(|v| v.as_str())
                                    {
                                        entry.arguments.push_str(arguments_delta);
                                        if tx
                                            .send(Ok(ToolStreamEvent::ToolCallDelta {
                                                index,
                                                arguments_delta: arguments_delta.to_string(),
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                }
                            }

                            if let Some(finish_reason) = choice
                                .and_then(|c| c.get("finish_reason"))
                                .and_then(|r| r.as_str())
                            {
                                final_finish_reason = Some(finish_reason.to_string());
                                for (index, acc) in accumulators.drain() {
                                    let arguments = serde_json::from_str::<Value>(&acc.arguments)
                                        .unwrap_or_else(|_| Value::String(acc.arguments.clone()));
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
    use talos_llm_provider::{CacheControl, CacheControlKind, PromptCacheSupport};

    #[test]
    fn test_openai_config_builder() {
        let config = OpenAIConfig::new("test-api-key")
            .with_base_url("https://custom.openai.com/v1")
            .with_timeout(120);

        assert_eq!(config.api_key, "test-api-key");
        assert_eq!(config.base_url, "https://custom.openai.com/v1");
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are a helpful assistant"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];

        let openai_msgs = OpenAIProvider::convert_messages(messages);

        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, "system");
        assert_eq!(
            openai_msgs[0].content.as_deref(),
            Some("You are a helpful assistant")
        );
        assert_eq!(openai_msgs[1].role, "user");
        assert_eq!(openai_msgs[1].content.as_deref(), Some("Hello"));
        assert_eq!(openai_msgs[2].role, "assistant");
        assert_eq!(openai_msgs[2].content.as_deref(), Some("Hi there!"));
    }

    #[test]
    fn test_openai_request_serialization_with_tools() {
        let req = OpenAIRequest {
            model: "gpt-4o".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: Some("Hello".to_string()),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            top_p: None,
            stop: None,
            tools: Some(vec![OpenAITool {
                r#type: "function".to_string(),
                function: OpenAIFunctionDefinition {
                    name: "read".to_string(),
                    description: "Read file".to_string(),
                    parameters: Some(serde_json::json!({"type": "object"})),
                },
            }]),
            tool_choice: Some(OpenAIToolChoice::Simple("auto".to_string())),
            parallel_tool_calls: Some(true),
            reasoning_effort: None,
            stream: None,
            stream_options: None,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o\""));
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"tool_choice\":\"auto\""));
    }

    #[test]
    fn test_build_request_maps_native_thinking_effort() {
        let request = ChatRequest::builder()
            .model("gpt-4o")
            .messages(vec![ChatMessage::user("hello")])
            .native_thinking(NativeThinkingConfig {
                effort: NativeThinkingEffort::High,
                budget_tokens: Some(2048),
            })
            .build();

        let built = OpenAIProvider::build_request(request, false);

        assert_eq!(built.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn test_parse_tool_calls() {
        let calls = vec![OpenAIToolCall {
            id: "call_1".to_string(),
            r#type: "function".to_string(),
            function: OpenAIFunctionCall {
                name: "read".to_string(),
                arguments: "{\"path\":\"/tmp/a\"}".to_string(),
            },
        }];

        let parsed = OpenAIProvider::parse_tool_calls(Some(calls)).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "read");
        assert_eq!(parsed[0].arguments["path"], "/tmp/a");
    }

    #[test]
    fn test_provider_name() {
        let config = OpenAIConfig::new("test-key");
        let provider = OpenAIProvider::new(config);
        assert_eq!(provider.name(), "openai");
        assert_eq!(provider.default_model(), "gpt-4o");
        assert_eq!(
            provider.prompt_cache_support(),
            PromptCacheSupport::Unsupported
        );
    }

    #[test]
    fn test_openai_request_does_not_serialize_cache_control_for_unsupported_provider() {
        let mut msg = ChatMessage::user("Hello");
        msg.cache_control = Some(CacheControl {
            kind: CacheControlKind::Ephemeral,
        });
        let request = OpenAIProvider::build_request(
            ChatRequest::builder()
                .model("gpt-4o")
                .messages(vec![msg])
                .build(),
            false,
        );

        let json = serde_json::to_string(&request).expect("request should serialize");
        assert!(!json.contains("cache_control"));
    }

    #[test]
    fn test_native_thinking_support_is_model_gated() {
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key"));

        assert_eq!(
            provider.native_thinking_support("o3-mini"),
            NativeThinkingSupport::Supported
        );
        assert_eq!(
            provider.native_thinking_support("gpt-5"),
            NativeThinkingSupport::Supported
        );
        assert_eq!(
            provider.native_thinking_support("gpt-4o"),
            NativeThinkingSupport::Unsupported
        );
    }

    #[test]
    fn test_extract_reasoning_delta_ignores_bailian_reasoning_content() {
        let delta = serde_json::json!({
            "reasoning_content": "bailian-only"
        });
        assert!(OpenAIProvider::extract_reasoning_delta(&delta).is_none());
    }

    #[test]
    fn test_build_headers() {
        let config = OpenAIConfig::new("test-api-key");
        let provider = OpenAIProvider::new(config);
        let headers = provider.build_headers();

        assert!(headers.contains_key("authorization"));
        assert!(headers.contains_key("content-type"));

        let auth_header = headers.get("authorization").unwrap();
        assert_eq!(auth_header.to_str().unwrap(), "Bearer test-api-key");
    }

    #[test]
    fn test_convert_tools() {
        let defs = vec![ToolDefinition::new(
            "read",
            "Read file",
            Some(serde_json::json!({"type":"object"})),
        )];
        let converted = OpenAIProvider::convert_tools(Some(defs)).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].function.name, "read");
    }
}
