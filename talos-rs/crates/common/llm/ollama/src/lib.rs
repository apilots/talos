//! Talos LLM Ollama provider.

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Role,
    ToolChoice, ToolDefinition, ToolStream, ToolStreamEvent, Usage,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Ollama API configuration.
#[derive(Debug, Clone)]
pub struct OllamaConfig {
    pub base_url: String,
    pub timeout_secs: u64,
}

impl OllamaConfig {
    pub fn new() -> Self {
        Self {
            base_url: "http://localhost:11434".to_string(),
            timeout_secs: 120,
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

impl Default for OllamaConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaResponse {
    model: String,
    message: OllamaMessage,
    #[allow(dead_code)]
    done: bool,
    #[allow(dead_code)]
    total_duration: Option<u64>,
    #[allow(dead_code)]
    load_duration: Option<u64>,
    prompt_eval_count: Option<u32>,
    #[allow(dead_code)]
    prompt_eval_duration: Option<u64>,
    eval_count: Option<u32>,
    #[allow(dead_code)]
    eval_duration: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct OllamaStreamResponse {
    #[allow(dead_code)]
    model: String,
    message: Option<OllamaMessage>,
    done: bool,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAICompatRequest {
    model: String,
    messages: Vec<OpenAICompatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAICompatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<OpenAICompatToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAICompatStreamOptions>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAICompatStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAICompatMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAICompatToolCall>>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAICompatTool {
    r#type: String,
    function: OpenAICompatFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAICompatFunctionDefinition {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum OpenAICompatToolChoice {
    Simple(String),
    Named {
        r#type: String,
        function: OpenAICompatToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize)]
struct OpenAICompatToolChoiceFunction {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAICompatToolCall {
    id: String,
    r#type: String,
    function: OpenAICompatFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAICompatFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAICompatResponse {
    model: String,
    choices: Vec<OpenAICompatChoice>,
    #[serde(default)]
    usage: Option<OpenAICompatUsage>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAICompatChoice {
    message: OpenAICompatResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAICompatResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAICompatToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAICompatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// Ollama LLM provider.
pub struct OllamaProvider {
    config: OllamaConfig,
    client: Client,
}

impl OllamaProvider {
    pub fn new(config: OllamaConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
    }

    fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    fn openai_compat_url(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    }

    fn convert_messages(messages: Vec<ChatMessage>) -> (Option<String>, Vec<OllamaMessage>) {
        let mut system_prompt = None;
        let ollama_messages: Vec<OllamaMessage> = messages
            .into_iter()
            .filter_map(|msg| match msg.role {
                Role::System => {
                    system_prompt = Some(msg.content);
                    None
                }
                Role::User => Some(OllamaMessage {
                    role: "user".to_string(),
                    content: msg.content,
                }),
                Role::Assistant => Some(OllamaMessage {
                    role: "assistant".to_string(),
                    content: msg.content,
                }),
                Role::Tool => Some(OllamaMessage {
                    role: "user".to_string(),
                    content: msg.content,
                }),
            })
            .collect();

        (system_prompt, ollama_messages)
    }

    fn convert_openai_messages(messages: Vec<ChatMessage>) -> Vec<OpenAICompatMessage> {
        messages
            .into_iter()
            .map(|msg| OpenAICompatMessage {
                role: match msg.role {
                    Role::System => "system".to_string(),
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                    Role::Tool => "tool".to_string(),
                },
                content: Some(msg.content),
                tool_call_id: msg.tool_call_id,
                tool_calls: msg.tool_calls.map(|calls| {
                    calls
                        .into_iter()
                        .map(|call| OpenAICompatToolCall {
                            id: call.id,
                            r#type: "function".to_string(),
                            function: OpenAICompatFunctionCall {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        })
                        .collect()
                }),
            })
            .collect()
    }

    fn convert_openai_tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<OpenAICompatTool>> {
        tools.map(|defs| {
            defs.into_iter()
                .map(|tool| OpenAICompatTool {
                    r#type: "function".to_string(),
                    function: OpenAICompatFunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.parameters,
                    },
                })
                .collect()
        })
    }

    fn convert_openai_tool_choice(choice: Option<ToolChoice>) -> Option<OpenAICompatToolChoice> {
        match choice {
            Some(ToolChoice::Auto) => Some(OpenAICompatToolChoice::Simple("auto".to_string())),
            Some(ToolChoice::Required) => {
                Some(OpenAICompatToolChoice::Simple("required".to_string()))
            }
            Some(ToolChoice::None) => Some(OpenAICompatToolChoice::Simple("none".to_string())),
            Some(ToolChoice::Named { name }) => Some(OpenAICompatToolChoice::Named {
                r#type: "function".to_string(),
                function: OpenAICompatToolChoiceFunction { name },
            }),
            None => None,
        }
    }

    fn parse_openai_tool_calls(
        calls: Option<Vec<OpenAICompatToolCall>>,
    ) -> Option<Vec<FunctionCall>> {
        calls.map(|items| {
            items
                .into_iter()
                .map(|item| {
                    let args = serde_json::from_str::<Value>(&item.function.arguments)
                        .unwrap_or_else(|_| Value::String(item.function.arguments));
                    FunctionCall::new(item.id, item.function.name, args)
                })
                .collect()
        })
    }

    fn extract_reasoning_delta(delta: &Value) -> Option<String> {
        for key in ["reasoning", "reasoning_content", "thinking"] {
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

    fn build_ollama_options(request: &ChatRequest) -> OllamaOptions {
        OllamaOptions {
            temperature: request.temperature,
            top_p: request.top_p,
            num_predict: request.max_tokens.map(|t| t as i32),
            stop: request.stop.clone(),
        }
    }

    async fn send_native_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let options = Self::build_ollama_options(&request);
        let (system, messages) = Self::convert_messages(request.messages);

        let ollama_req = OllamaRequest {
            model: request.model,
            messages,
            system,
            stream: Some(false),
            options: Some(options),
        };

        let url = format!("{}/api/chat", self.config.base_url);
        let headers = self.build_headers();

        debug!("Request URL: {}", url);
        debug!("Request model: {}", ollama_req.model);

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&ollama_req)
            .send()
            .await
            .map_err(|e| TalosError::InternalError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("Ollama API error: {} - {}", status, error_text);
            return Err(TalosError::InternalError(format!(
                "Ollama API error: {} - {}",
                status, error_text
            )));
        }

        let ollama_resp: OllamaResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        Ok(ChatResponse {
            content: ollama_resp.message.content,
            reasoning: Vec::new(),
            model: ollama_resp.model,
            usage: Usage {
                prompt_tokens: ollama_resp.prompt_eval_count.unwrap_or(0),
                completion_tokens: ollama_resp.eval_count.unwrap_or(0),
                total_tokens: ollama_resp.prompt_eval_count.unwrap_or(0)
                    + ollama_resp.eval_count.unwrap_or(0),
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn send_openai_compat_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let compat_req = OpenAICompatRequest {
            model: request.model,
            messages: Self::convert_openai_messages(request.messages),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stop: request.stop,
            tools: Self::convert_openai_tools(request.tools),
            tool_choice: Self::convert_openai_tool_choice(request.tool_choice),
            parallel_tool_calls: request.parallel_tool_calls,
            stream: Some(false),
            stream_options: None,
        };

        let url = self.openai_compat_url();
        let response = self
            .client
            .post(&url)
            .headers(self.build_headers())
            .json(&compat_req)
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
                "Ollama OpenAI-compat API error: {} - {}",
                status, error_text
            )));
        }

        let compat_resp: OpenAICompatResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        let choice = compat_resp.choices.into_iter().next().ok_or_else(|| {
            TalosError::InternalError("No choices in Ollama OpenAI-compat response".to_string())
        })?;

        let usage = compat_resp.usage.unwrap_or(OpenAICompatUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        });

        Ok(ChatResponse {
            content: choice.message.content.unwrap_or_default(),
            reasoning: Vec::new(),
            model: compat_resp.model,
            usage: Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            finish_reason: choice.finish_reason,
            tool_calls: Self::parse_openai_tool_calls(choice.message.tool_calls),
        })
    }

    async fn send_native_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let options = Self::build_ollama_options(&request);
        let (system, messages) = Self::convert_messages(request.messages);

        let ollama_req = OllamaRequest {
            model: request.model,
            messages,
            system,
            stream: Some(true),
            options: Some(options),
        };

        let url = format!("{}/api/chat", self.config.base_url);
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&ollama_req)
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
                "Ollama API error: {} - {}",
                status, error_text
            )));
        }

        Ok(response)
    }

    async fn send_openai_compat_stream(
        &self,
        request: ChatRequest,
    ) -> TalosResult<reqwest::Response> {
        let compat_req = OpenAICompatRequest {
            model: request.model,
            messages: Self::convert_openai_messages(request.messages),
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            top_p: request.top_p,
            stop: request.stop,
            tools: Self::convert_openai_tools(request.tools),
            tool_choice: Self::convert_openai_tool_choice(request.tool_choice),
            parallel_tool_calls: request.parallel_tool_calls,
            stream: Some(true),
            stream_options: Some(OpenAICompatStreamOptions {
                include_usage: true,
            }),
        };

        let response = self
            .client
            .post(self.openai_compat_url())
            .headers(self.build_headers())
            .json(&compat_req)
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
                "Ollama OpenAI-compat API error: {} - {}",
                status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn default_model(&self) -> &str {
        "llama3"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to Ollama API");
        if request.tools.is_some() {
            self.send_openai_compat_chat(request).await
        } else {
            self.send_native_chat(request).await
        }
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending tool-aware chat request to Ollama API");
        if request.tools.is_some() {
            self.send_openai_compat_chat(request).await
        } else {
            self.send_native_chat(request).await
        }
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = if request.tools.is_some() {
            self.send_openai_compat_stream(request).await?
        } else {
            self.send_native_stream(request).await?
        };

        let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }

                            if line.starts_with("data: ") {
                                let data = &line[6..];
                                if data == "[DONE]" {
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: String::new(),
                                            finish_reason: Some("stop".to_string()),
                                        }))
                                        .await;
                                    return;
                                }
                                if let Ok(event) = serde_json::from_str::<Value>(data) {
                                    if let Some(choice) = event
                                        .get("choices")
                                        .and_then(|c| c.as_array())
                                        .and_then(|arr| arr.first())
                                    {
                                        if let Some(delta) = choice
                                            .get("delta")
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
                                        if let Some(reason) =
                                            choice.get("finish_reason").and_then(|v| v.as_str())
                                        {
                                            let _ = tx
                                                .send(Ok(ChatChunk {
                                                    delta: String::new(),
                                                    finish_reason: Some(reason.to_string()),
                                                }))
                                                .await;
                                            return;
                                        }
                                    }
                                }
                                continue;
                            }

                            if let Ok(stream_resp) =
                                serde_json::from_str::<OllamaStreamResponse>(line)
                            {
                                if let Some(msg) = &stream_resp.message {
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: msg.content.clone(),
                                            finish_reason: None,
                                        }))
                                        .await;
                                }
                                if stream_resp.done {
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: String::new(),
                                            finish_reason: Some("stop".to_string()),
                                        }))
                                        .await;
                                    return;
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
        if request.tools.is_none() {
            let mut plain_stream = self.stream(request).await?;
            let (tx, rx) = mpsc::channel::<TalosResult<ToolStreamEvent>>(100);
            tokio::spawn(async move {
                while let Some(item) = plain_stream.next().await {
                    match item {
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
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                    }
                }
            });
            return Ok(ToolStream::new(rx));
        }

        let response = self.send_openai_compat_stream(request).await?;
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
                                serde_json::from_value::<OpenAICompatUsage>(value.clone()).ok()
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

                            if let Some(reason) = choice
                                .and_then(|c| c.get("finish_reason"))
                                .and_then(|r| r.as_str())
                            {
                                final_finish_reason = Some(reason.to_string());
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
    fn test_ollama_config_builder() {
        let config = OllamaConfig::new()
            .with_base_url("http://custom-host:11434")
            .with_timeout(180);

        assert_eq!(config.base_url, "http://custom-host:11434");
        assert_eq!(config.timeout_secs, 180);
    }

    #[test]
    fn test_ollama_config_default() {
        let config = OllamaConfig::default();
        assert_eq!(config.base_url, "http://localhost:11434");
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are a helpful assistant"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];

        let (system, ollama_msgs) = OllamaProvider::convert_messages(messages);

        assert_eq!(system, Some("You are a helpful assistant".to_string()));
        assert_eq!(ollama_msgs.len(), 2);
        assert_eq!(ollama_msgs[0].role, "user");
        assert_eq!(ollama_msgs[0].content, "Hello");
        assert_eq!(ollama_msgs[1].role, "assistant");
        assert_eq!(ollama_msgs[1].content, "Hi there!");
    }

    #[test]
    fn test_openai_compat_url() {
        let provider =
            OllamaProvider::new(OllamaConfig::new().with_base_url("http://localhost:11434"));
        assert_eq!(
            provider.openai_compat_url(),
            "http://localhost:11434/v1/chat/completions"
        );

        let provider_v1 =
            OllamaProvider::new(OllamaConfig::new().with_base_url("http://localhost:11434/v1"));
        assert_eq!(
            provider_v1.openai_compat_url(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_headers() {
        let config = OllamaConfig::new();
        let provider = OllamaProvider::new(config);
        let headers = provider.build_headers();

        assert!(headers.contains_key("content-type"));

        let content_type = headers.get("content-type").unwrap();
        assert_eq!(content_type.to_str().unwrap(), "application/json");
    }

    #[test]
    fn test_build_ollama_options() {
        let request = ChatRequest::builder()
            .message("Hello")
            .model("llama3")
            .temperature(0.7)
            .max_tokens(100)
            .top_p(0.9)
            .stop(vec!["\n".to_string()])
            .build();

        let options = OllamaProvider::build_ollama_options(&request);

        assert_eq!(options.temperature, Some(0.7));
        assert_eq!(options.top_p, Some(0.9));
        assert_eq!(options.num_predict, Some(100));
        assert_eq!(options.stop, Some(vec!["\n".to_string()]));
    }

    #[test]
    fn test_openai_tool_conversion() {
        let defs = vec![ToolDefinition::new(
            "read",
            "Read file",
            Some(serde_json::json!({"type": "object"})),
        )];
        let converted = OllamaProvider::convert_openai_tools(Some(defs)).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].function.name, "read");
    }

    #[test]
    fn test_ollama_stream_response_deserialization() {
        let json = r#"{
            "model": "llama3",
            "message": {
                "role": "assistant",
                "content": "Hello!"
            },
            "done": false,
            "prompt_eval_count": 10,
            "eval_count": 5
        }"#;

        let response: OllamaStreamResponse = serde_json::from_str(json).unwrap();
        assert!(!response.done);
        assert!(response.message.is_some());
        assert_eq!(response.prompt_eval_count, Some(10));
        assert_eq!(response.eval_count, Some(5));
    }

    #[test]
    fn test_ollama_request_does_not_serialize_cache_control_for_unsupported_provider() {
        let provider = OllamaProvider::new(OllamaConfig::new());
        assert_eq!(
            provider.prompt_cache_support(),
            PromptCacheSupport::Unsupported
        );

        let mut message = ChatMessage::user("hello");
        message.cache_control = Some(CacheControl {
            kind: CacheControlKind::Ephemeral,
        });

        let request = OpenAICompatRequest {
            model: "llama3".to_string(),
            messages: OllamaProvider::convert_openai_messages(vec![message]),
            temperature: None,
            max_tokens: None,
            top_p: None,
            stop: None,
            tools: None,
            tool_choice: None,
            parallel_tool_calls: None,
            stream: Some(false),
            stream_options: None,
        };

        let json = serde_json::to_string(&request).expect("request should serialize");
        assert!(!json.contains("cache_control"));
    }
}
