//! Talos LLM OpenAI provider.

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider,
    ModelBlockCompleted, ModelBlockDelta, ModelBlockKind, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, ModelToolCallStart, NativeThinkingConfig, NativeThinkingEffort,
    NativeThinkingSupport, Role, SseLineDecoder, ToolChoice, ToolDefinition, Usage,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use uuid::Uuid;

static CODEX_INSTALLATION_ID: LazyLock<String> =
    LazyLock::new(|| format!("talos-codex-{}", Uuid::new_v4().simple()));
static CODEX_CONVERSATION_ID: LazyLock<String> =
    LazyLock::new(|| format!("talos-session-{}", Uuid::new_v4().simple()));

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

#[derive(Debug, Default)]
struct OpenAIModelBlockStreamState {
    reasoning_started: HashMap<usize, bool>,
    assistant_started: HashMap<usize, bool>,
    tool_started: HashMap<usize, bool>,
}

#[derive(Debug, Default)]
struct OpenAIModelToolAcc {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAIEndpointKind {
    AliyunCodingPlan,
    Compatible,
}

/// OpenAI LLM provider.
pub struct OpenAIProvider {
    config: OpenAIConfig,
    client: Client,
}

impl OpenAIProvider {
    pub fn new(config: OpenAIConfig) -> Self {
        let mut builder = Client::builder().timeout(Duration::from_secs(config.timeout_secs));

        if Self::endpoint_kind_for_base_url(&config.base_url)
            == OpenAIEndpointKind::AliyunCodingPlan
        {
            builder = builder.no_gzip().no_brotli().no_zstd().no_deflate();
        }

        let client = builder.build().expect("Failed to build HTTP client");

        Self { config, client }
    }

    fn endpoint_kind_for_base_url(base_url: &str) -> OpenAIEndpointKind {
        let base_url = base_url.trim_end_matches('/');
        if base_url.contains("coding.dashscope.aliyuncs.com")
            || base_url.contains("coding-intl.dashscope.aliyuncs.com")
        {
            OpenAIEndpointKind::AliyunCodingPlan
        } else {
            OpenAIEndpointKind::Compatible
        }
    }

    fn endpoint_kind(&self) -> OpenAIEndpointKind {
        Self::endpoint_kind_for_base_url(&self.config.base_url)
    }

    fn insert_header_value(
        headers: &mut header::HeaderMap,
        name: &'static str,
        value: impl AsRef<str>,
    ) {
        if let Ok(header_value) = header::HeaderValue::from_str(value.as_ref()) {
            headers.insert(name, header_value);
        }
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

        if self.endpoint_kind() == OpenAIEndpointKind::AliyunCodingPlan {
            let conversation_id = CODEX_CONVERSATION_ID.as_str();
            let window_id = format!("{conversation_id}-1");
            let user_agent = format!("codex_cli_rs/{}", env!("CARGO_PKG_VERSION"));

            headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
            headers.insert(
                "originator",
                header::HeaderValue::from_static("codex_cli_rs"),
            );
            Self::insert_header_value(&mut headers, header::USER_AGENT.as_str(), user_agent);
            headers.insert(
                "version",
                header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
            );
            Self::insert_header_value(
                &mut headers,
                "x-codex-installation-id",
                CODEX_INSTALLATION_ID.as_str(),
            );
            Self::insert_header_value(&mut headers, "x-codex-window-id", &window_id);
            Self::insert_header_value(&mut headers, "session_id", conversation_id);
            Self::insert_header_value(&mut headers, "x-client-request-id", conversation_id);
        }

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

    fn choice_index(choice: Option<&Value>) -> usize {
        choice
            .and_then(|c| c.get("index"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize
    }

    fn reasoning_model_item_id(choice_index: usize) -> String {
        format!("openai-choice-{choice_index}-reasoning")
    }

    fn assistant_model_item_id(choice_index: usize) -> String {
        format!("openai-choice-{choice_index}-assistant")
    }

    fn tool_model_item_id(index: usize) -> String {
        format!("openai-tool-{index}")
    }

    async fn send_model_block_event(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        event: ModelBlockStreamEvent,
    ) -> bool {
        tx.send(Ok(event)).await.is_ok()
    }

    async fn ensure_reasoning_model_block(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        state: &mut OpenAIModelBlockStreamState,
        choice_index: usize,
    ) -> bool {
        if state.reasoning_started.get(&choice_index) == Some(&true) {
            return true;
        }
        state.reasoning_started.insert(choice_index, true);
        Self::send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
                model_item_id: Self::reasoning_model_item_id(choice_index),
                provider_item_id: Some(format!("choice:{choice_index}:reasoning")),
                kind: ModelBlockKind::Reasoning,
                assistant_text_phase_hint: None,
                assistant_text_phase_hint_source: None,
                tool_call: None,
            }),
        )
        .await
    }

    async fn ensure_assistant_model_block(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        state: &mut OpenAIModelBlockStreamState,
        choice_index: usize,
    ) -> bool {
        if state.assistant_started.get(&choice_index) == Some(&true) {
            return true;
        }
        state.assistant_started.insert(choice_index, true);
        Self::send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
                model_item_id: Self::assistant_model_item_id(choice_index),
                provider_item_id: Some(format!("choice:{choice_index}:message")),
                kind: ModelBlockKind::AssistantText,
                assistant_text_phase_hint: None,
                assistant_text_phase_hint_source: None,
                tool_call: None,
            }),
        )
        .await
    }

    async fn ensure_tool_model_block(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        state: &mut OpenAIModelBlockStreamState,
        index: usize,
        acc: &OpenAIModelToolAcc,
    ) -> bool {
        if state.tool_started.get(&index) == Some(&true) {
            return true;
        }
        state.tool_started.insert(index, true);
        Self::send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
                model_item_id: Self::tool_model_item_id(index),
                provider_item_id: acc.id.clone(),
                kind: ModelBlockKind::ToolCall,
                assistant_text_phase_hint: None,
                assistant_text_phase_hint_source: None,
                tool_call: Some(ModelToolCallStart {
                    index,
                    call_id: acc.id.clone(),
                    tool_name: acc.name.clone(),
                }),
            }),
        )
        .await
    }

    async fn complete_open_model_blocks(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        state: &mut OpenAIModelBlockStreamState,
    ) -> bool {
        let reasoning_ids: Vec<_> = state
            .reasoning_started
            .iter()
            .filter_map(|(index, started)| started.then_some(*index))
            .collect();
        for index in reasoning_ids {
            state.reasoning_started.insert(index, false);
            if !Self::send_model_block_event(
                tx,
                ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                    model_item_id: Self::reasoning_model_item_id(index),
                }),
            )
            .await
            {
                return false;
            }
        }

        let assistant_ids: Vec<_> = state
            .assistant_started
            .iter()
            .filter_map(|(index, started)| started.then_some(*index))
            .collect();
        for index in assistant_ids {
            state.assistant_started.insert(index, false);
            if !Self::send_model_block_event(
                tx,
                ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                    model_item_id: Self::assistant_model_item_id(index),
                }),
            )
            .await
            {
                return false;
            }
        }

        let tool_ids: Vec<_> = state
            .tool_started
            .iter()
            .filter_map(|(index, started)| started.then_some(*index))
            .collect();
        for index in tool_ids {
            state.tool_started.insert(index, false);
            if !Self::send_model_block_event(
                tx,
                ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                    model_item_id: Self::tool_model_item_id(index),
                }),
            )
            .await
            {
                return false;
            }
        }
        true
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
            let mut decoder = SseLineDecoder::new();
            let mut final_finish_reason: Option<String> = None;
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        for line in decoder.push(&bytes) {
                            let line = match line {
                                Ok(line) => line,
                                Err(err) => {
                                    let _ = tx.send(Err(err)).await;
                                    return;
                                }
                            };
                            let Some(data) = line.strip_prefix("data:") else {
                                continue;
                            };

                            let data = data.trim_start();
                            if data.is_empty() {
                                continue;
                            }
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

                            let event = match serde_json::from_str::<Value>(data) {
                                Ok(event) => event,
                                Err(error) => {
                                    let _ = tx
                                        .send(Err(TalosError::InternalError(format!(
                                            "OpenAI stream invalid SSE JSON: {error}"
                                        ))))
                                        .await;
                                    return;
                                }
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

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(100);

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut decoder = SseLineDecoder::new();
            let mut state = OpenAIModelBlockStreamState::default();
            let mut tool_acc: HashMap<usize, OpenAIModelToolAcc> = HashMap::new();
            let mut final_finish_reason: Option<String> = None;
            let mut final_usage: Option<Usage> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        for line in decoder.push(&bytes) {
                            let line = match line {
                                Ok(line) => line,
                                Err(err) => {
                                    let _ = tx.send(Err(err)).await;
                                    return;
                                }
                            };
                            let Some(data) = line.strip_prefix("data:") else {
                                continue;
                            };

                            let data = data.trim_start();
                            if data.is_empty() {
                                continue;
                            }

                            if data == "[DONE]" {
                                if !Self::complete_open_model_blocks(&tx, &mut state).await {
                                    return;
                                }
                                let _ = tx
                                    .send(Ok(ModelBlockStreamEvent::Done {
                                        finish_reason: final_finish_reason
                                            .clone()
                                            .or_else(|| Some("stop".to_string())),
                                        usage: final_usage.clone(),
                                    }))
                                    .await;
                                return;
                            }

                            let event = match serde_json::from_str::<Value>(data) {
                                Ok(event) => event,
                                Err(error) => {
                                    let _ = tx
                                        .send(Err(TalosError::InternalError(format!(
                                            "OpenAI model block stream invalid SSE JSON: {error}"
                                        ))))
                                        .await;
                                    return;
                                }
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
                            let choice_index = Self::choice_index(choice);
                            let delta_object = choice.and_then(|c| c.get("delta"));

                            if let Some(reasoning_delta) =
                                delta_object.and_then(Self::extract_reasoning_delta)
                            {
                                if !Self::ensure_reasoning_model_block(
                                    &tx,
                                    &mut state,
                                    choice_index,
                                )
                                .await
                                    || !Self::send_model_block_event(
                                        &tx,
                                        ModelBlockStreamEvent::BlockDelta {
                                            model_item_id: Self::reasoning_model_item_id(
                                                choice_index,
                                            ),
                                            delta: ModelBlockDelta::ReasoningText {
                                                delta: reasoning_delta,
                                            },
                                        },
                                    )
                                    .await
                                {
                                    return;
                                }
                            }

                            if let Some(delta) = delta_object
                                .and_then(|d| d.get("content"))
                                .and_then(|t| t.as_str())
                            {
                                if !Self::ensure_assistant_model_block(
                                    &tx,
                                    &mut state,
                                    choice_index,
                                )
                                .await
                                    || !Self::send_model_block_event(
                                        &tx,
                                        ModelBlockStreamEvent::BlockDelta {
                                            model_item_id: Self::assistant_model_item_id(
                                                choice_index,
                                            ),
                                            delta: ModelBlockDelta::AssistantText {
                                                delta: delta.to_string(),
                                            },
                                        },
                                    )
                                    .await
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
                                    let mut should_start = false;
                                    {
                                        let entry = tool_acc.entry(index).or_default();
                                        if let Some(id) = tool_call_delta
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .map(str::to_string)
                                        {
                                            entry.id = Some(id);
                                            should_start = true;
                                        }
                                        if let Some(name) = tool_call_delta
                                            .get("function")
                                            .and_then(|f| f.get("name"))
                                            .and_then(|v| v.as_str())
                                            .map(str::to_string)
                                        {
                                            entry.name = Some(name);
                                            should_start = true;
                                        }
                                    }

                                    let arguments_delta = tool_call_delta
                                        .get("function")
                                        .and_then(|f| f.get("arguments"))
                                        .and_then(|v| v.as_str())
                                        .map(str::to_string);
                                    if arguments_delta.is_some() {
                                        should_start = true;
                                    }

                                    if should_start {
                                        let entry = tool_acc.entry(index).or_default();
                                        if !Self::ensure_tool_model_block(
                                            &tx, &mut state, index, entry,
                                        )
                                        .await
                                        {
                                            return;
                                        }
                                    }

                                    if let Some(arguments_delta) = arguments_delta {
                                        let entry = tool_acc.entry(index).or_default();
                                        entry.arguments.push_str(&arguments_delta);
                                        if !Self::send_model_block_event(
                                            &tx,
                                            ModelBlockStreamEvent::BlockDelta {
                                                model_item_id: Self::tool_model_item_id(index),
                                                delta: ModelBlockDelta::ToolArguments {
                                                    delta: arguments_delta,
                                                },
                                            },
                                        )
                                        .await
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
                                if !Self::complete_open_model_blocks(&tx, &mut state).await {
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

        Ok(ModelBlockStream::new(rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_llm_provider::{CacheControl, CacheControlKind, PromptCacheSupport};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_sse_server(body: String) -> String {
        spawn_sse_server_chunks(vec![body]).await
    }

    async fn spawn_sse_server_chunks(chunks: Vec<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server addr should exist");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request should arrive");
            let mut buf = vec![0; 4096];
            let _ = socket.read(&mut buf).await.expect("request should read");
            let content_length: usize = chunks.iter().map(|chunk| chunk.len()).sum();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                content_length
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("headers should write");
            for chunk in chunks {
                socket
                    .write_all(chunk.as_bytes())
                    .await
                    .expect("chunk should write");
            }
        });
        format!("http://{addr}")
    }

    async fn collect_model_events(mut stream: ModelBlockStream) -> Vec<ModelBlockStreamEvent> {
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            let event = event.expect("model block event should succeed");
            let done = matches!(event, ModelBlockStreamEvent::Done { .. });
            events.push(event);
            if done {
                break;
            }
        }
        events
    }

    async fn collect_first_model_error(mut stream: ModelBlockStream) -> TalosError {
        while let Some(event) = stream.next().await {
            if let Err(err) = event {
                return err;
            }
        }
        panic!("stream should emit an error");
    }

    #[test]
    fn test_openai_config_builder() {
        let config = OpenAIConfig::new("test-api-key")
            .with_base_url("https://custom.openai.com/v1")
            .with_timeout(120);

        assert_eq!(config.api_key, "test-api-key");
        assert_eq!(config.base_url, "https://custom.openai.com/v1");
        assert_eq!(config.timeout_secs, 120);
    }

    #[tokio::test]
    async fn test_stream_model_blocks_maps_chat_completion_chunks() {
        let body = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"think\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"/tmp/a\\\"}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
            "data: [DONE]\n\n"
        )
        .to_string();
        let base_url = spawn_sse_server(body).await;
        let provider =
            OpenAIProvider::new(OpenAIConfig::new("test-api-key").with_base_url(base_url));
        let stream = provider
            .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
            .await
            .expect("stream should start");

        let events = collect_model_events(stream).await;

        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockStarted(started)
                if started.model_item_id == "openai-choice-0-reasoning"
                    && started.kind == ModelBlockKind::Reasoning
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-choice-0-assistant" && delta == "hello"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockStarted(started)
                if started.model_item_id == "openai-tool-0"
                    && started.provider_item_id.as_deref() == Some("call_1")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockCompleted(completed)
                if completed.model_item_id == "openai-tool-0"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::Done { usage: Some(usage), .. }
                if usage.total_tokens == 3
        )));
    }

    #[tokio::test]
    async fn test_stream_model_blocks_buffers_split_sse_lines() {
        let chunks = vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he".to_string(),
            "llo\"}}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let base_url = spawn_sse_server_chunks(chunks).await;
        let provider =
            OpenAIProvider::new(OpenAIConfig::new("test-api-key").with_base_url(base_url));
        let stream = provider
            .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
            .await
            .expect("stream should start");

        let events = collect_model_events(stream).await;

        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-choice-0-assistant" && delta == "hello"
        )));
    }

    #[tokio::test]
    async fn test_stream_model_blocks_fails_invalid_sse_json() {
        let base_url = spawn_sse_server("data: {not-json}\n\n".to_string()).await;
        let provider =
            OpenAIProvider::new(OpenAIConfig::new("test-api-key").with_base_url(base_url));
        let stream = provider
            .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
            .await
            .expect("stream should start");

        let err = collect_first_model_error(stream).await;

        assert!(format!("{err}").contains("invalid SSE JSON"));
    }

    #[test]
    fn test_coding_plan_endpoint_detection() {
        assert_eq!(
            OpenAIProvider::endpoint_kind_for_base_url("https://coding.dashscope.aliyuncs.com/v1"),
            OpenAIEndpointKind::AliyunCodingPlan
        );
        assert_eq!(
            OpenAIProvider::endpoint_kind_for_base_url(
                "https://coding-intl.dashscope.aliyuncs.com/v1/"
            ),
            OpenAIEndpointKind::AliyunCodingPlan
        );
        assert_eq!(
            OpenAIProvider::endpoint_kind_for_base_url("https://api.openai.com/v1"),
            OpenAIEndpointKind::Compatible
        );
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
        assert!(!headers.contains_key("originator"));
        assert!(!headers.contains_key("x-codex-installation-id"));
    }

    #[test]
    fn test_build_headers_adds_coding_plan_fingerprint() {
        let config = OpenAIConfig::new("test-api-key")
            .with_base_url("https://coding.dashscope.aliyuncs.com/v1");
        let provider = OpenAIProvider::new(config);
        let headers = provider.build_headers();

        assert_eq!(
            headers
                .get("originator")
                .and_then(|value| value.to_str().ok()),
            Some("codex_cli_rs")
        );
        assert_eq!(
            headers.get("accept").and_then(|value| value.to_str().ok()),
            Some("*/*")
        );
        assert_eq!(
            headers.get("version").and_then(|value| value.to_str().ok()),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert!(headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("codex_cli_rs/")));
        assert!(headers.contains_key("x-codex-installation-id"));
        assert!(headers.contains_key("x-codex-window-id"));
        assert!(headers.contains_key("session_id"));
        assert!(headers.contains_key("x-client-request-id"));
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
