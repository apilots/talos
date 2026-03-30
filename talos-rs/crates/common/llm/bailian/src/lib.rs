//! Talos LLM Bailian provider.

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
    NativeThinkingSupport, Role, ToolChoice, ToolDefinition, ToolStream, ToolStreamEvent, Usage,
};
use tokio::sync::mpsc;
use tracing::{error, info};

/// Bailian API configuration.
#[derive(Debug, Clone)]
pub struct BailianConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
}

impl BailianConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BailianThinkingMode {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BailianThinkingConfig {
    pub mode: BailianThinkingMode,
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BailianModelCapability {
    Basic,
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BailianMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<BailianToolCall>>,
}

#[derive(Debug, Clone, Serialize)]
struct BailianTool {
    r#type: String,
    function: BailianFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct BailianFunctionDefinition {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BailianToolCall {
    id: String,
    r#type: String,
    function: BailianFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BailianFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum BailianToolChoice {
    Simple(String),
    Named {
        r#type: String,
        function: BailianToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize)]
struct BailianToolChoiceFunction {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
struct BailianRequest {
    model: String,
    messages: Vec<BailianMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<BailianTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<BailianToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<BailianStreamOptions>,
}

#[derive(Debug, Clone, Serialize)]
struct BailianStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct BailianResponse {
    model: String,
    choices: Vec<BailianChoice>,
    #[serde(default)]
    usage: Option<BailianUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct BailianChoice {
    message: BailianResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct BailianResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<BailianToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct BailianUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// Bailian LLM provider.
pub struct BailianProvider {
    config: BailianConfig,
    client: Client,
}

impl BailianProvider {
    pub fn new(config: BailianConfig) -> Self {
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

    fn convert_tool_choice(choice: Option<ToolChoice>) -> Option<BailianToolChoice> {
        match choice {
            Some(ToolChoice::Auto) => Some(BailianToolChoice::Simple("auto".to_string())),
            Some(ToolChoice::Required) => Some(BailianToolChoice::Simple("required".to_string())),
            Some(ToolChoice::None) => Some(BailianToolChoice::Simple("none".to_string())),
            Some(ToolChoice::Named { name }) => Some(BailianToolChoice::Named {
                r#type: "function".to_string(),
                function: BailianToolChoiceFunction { name },
            }),
            None => None,
        }
    }

    fn convert_tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<BailianTool>> {
        tools.map(|definitions| {
            definitions
                .into_iter()
                .map(|tool| BailianTool {
                    r#type: "function".to_string(),
                    function: BailianFunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        })
    }

    fn convert_messages(messages: Vec<ChatMessage>) -> Vec<BailianMessage> {
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
                        .map(|call| BailianToolCall {
                            id: call.id,
                            r#type: "function".to_string(),
                            function: BailianFunctionCall {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        })
                        .collect()
                });

                BailianMessage {
                    role,
                    content: Some(msg.content),
                    name: msg.name,
                    tool_call_id: msg.tool_call_id,
                    tool_calls,
                }
            })
            .collect()
    }

    fn parse_tool_calls(tool_calls: Option<Vec<BailianToolCall>>) -> Option<Vec<FunctionCall>> {
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

    fn model_capability(model: &str) -> BailianModelCapability {
        let normalized = model.trim().to_ascii_lowercase();
        if normalized.starts_with("qwen-plus")
            || normalized.starts_with("qwen3-max")
            || normalized.starts_with("qwq")
        {
            BailianModelCapability::Thinking
        } else {
            BailianModelCapability::Basic
        }
    }

    fn native_thinking_config(request: &ChatRequest) -> Option<BailianThinkingConfig> {
        request
            .native_thinking
            .as_ref()
            .map(|config| BailianThinkingConfig {
                mode: BailianThinkingMode::Enabled,
                budget_tokens: config.budget_tokens,
            })
    }

    fn extract_reasoning_delta(delta: &Value) -> Option<String> {
        for key in ["reasoning_content", "thinking", "reasoning"] {
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

    fn build_request(request: ChatRequest, stream: bool) -> BailianRequest {
        let thinking = Self::native_thinking_config(&request);
        BailianRequest {
            model: request.model,
            messages: Self::convert_messages(request.messages),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stop: request.stop,
            tools: Self::convert_tools(request.tools),
            tool_choice: Self::convert_tool_choice(request.tool_choice),
            parallel_tool_calls: request.parallel_tool_calls,
            enable_thinking: thinking
                .as_ref()
                .map(|config| matches!(config.mode, BailianThinkingMode::Enabled)),
            thinking_budget: thinking.as_ref().and_then(|config| config.budget_tokens),
            stream: Some(stream),
            stream_options: if stream {
                Some(BailianStreamOptions {
                    include_usage: true,
                })
            } else {
                None
            },
        }
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let bailian_req = Self::build_request(request, false);
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&bailian_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("Bailian API error: {} - {}", status, error_text);
            return Err(TalosError::InternalError(format!(
                "Bailian API error: {} - {}",
                status, error_text
            )));
        }

        let bailian_resp: BailianResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        let choice = bailian_resp.choices.into_iter().next().ok_or_else(|| {
            TalosError::InternalError("No choices in Bailian response".to_string())
        })?;

        let usage = bailian_resp.usage.unwrap_or(BailianUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        });

        Ok(ChatResponse {
            content: choice.message.content.unwrap_or_default(),
            model: bailian_resp.model,
            usage: Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            },
            finish_reason: choice.finish_reason,
            tool_calls: Self::parse_tool_calls(choice.message.tool_calls),
        })
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let bailian_req = Self::build_request(request, true);
        let url = format!("{}/chat/completions", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&bailian_req)
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
                "Bailian API error: {} - {}",
                status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for BailianProvider {
    fn name(&self) -> &str {
        "bailian"
    }

    fn default_model(&self) -> &str {
        "qwen-plus"
    }

    fn native_thinking_support(&self, model: &str) -> NativeThinkingSupport {
        match Self::model_capability(model) {
            BailianModelCapability::Thinking => NativeThinkingSupport::Supported,
            BailianModelCapability::Basic => NativeThinkingSupport::Unsupported,
        }
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to Bailian API");
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to Bailian API");
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

                            let Ok(event) = serde_json::from_str::<Value>(data) else {
                                continue;
                            };

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
                                serde_json::from_value::<BailianUsage>(value.clone()).ok()
                            }) {
                                final_usage = Some(Usage {
                                    prompt_tokens: usage.prompt_tokens,
                                    completion_tokens: usage.completion_tokens,
                                    total_tokens: usage.total_tokens,
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

                            if let Some(tool_calls) = delta_object
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
                                        .map(str::to_string)
                                    {
                                        entry.id = Some(id);
                                    }

                                    if let Some(name) = tool_call_delta
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|v| v.as_str())
                                        .map(str::to_string)
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
    use talos_llm_provider::{NativeThinkingConfig, NativeThinkingEffort, ToolStreamEvent};

    #[test]
    fn test_bailian_config_builder() {
        let config = BailianConfig::new("test-api-key")
            .with_base_url("https://example.com/compatible-mode/v1")
            .with_timeout(120);

        assert_eq!(config.api_key, "test-api-key");
        assert_eq!(config.base_url, "https://example.com/compatible-mode/v1");
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_native_thinking_support_is_qwen_gated() {
        let provider = BailianProvider::new(BailianConfig::new("test-key"));
        assert_eq!(
            provider.native_thinking_support("qwen-plus"),
            NativeThinkingSupport::Supported
        );
        assert_eq!(
            provider.native_thinking_support("qwen-plus-latest"),
            NativeThinkingSupport::Supported
        );
        assert_eq!(
            provider.native_thinking_support("qwen-max"),
            NativeThinkingSupport::Unsupported
        );
    }

    #[test]
    fn test_build_request_maps_native_thinking_to_bailian_fields() {
        let request = ChatRequest::builder()
            .model("qwen-plus")
            .messages(vec![ChatMessage::user("hello")])
            .native_thinking(NativeThinkingConfig {
                effort: NativeThinkingEffort::Medium,
                budget_tokens: Some(1024),
            })
            .build();

        let built = BailianProvider::build_request(request, true);
        assert_eq!(built.enable_thinking, Some(true));
        assert_eq!(built.thinking_budget, Some(1024));
        assert!(built.stream.unwrap_or(false));
    }

    #[test]
    fn test_extract_reasoning_delta_reads_reasoning_content() {
        let delta = serde_json::json!({
            "reasoning_content": [
                {"text": "first "},
                {"content": "second"}
            ]
        });
        assert_eq!(
            BailianProvider::extract_reasoning_delta(&delta).as_deref(),
            Some("first second")
        );
    }

    #[tokio::test]
    async fn test_stream_done_event_shape_is_preserved() {
        let (tx, rx) = mpsc::channel(1);
        tx.send(Ok(ToolStreamEvent::Done {
            finish_reason: Some("stop".to_string()),
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 2,
                total_tokens: 3,
            }),
        }))
        .await
        .expect("send should succeed");

        let mut stream = ToolStream::new(rx);
        let event = stream
            .next()
            .await
            .expect("event should exist")
            .expect("event should be ok");
        assert!(matches!(
            event,
            ToolStreamEvent::Done {
                finish_reason: Some(_),
                usage: Some(_)
            }
        ));
    }
}
