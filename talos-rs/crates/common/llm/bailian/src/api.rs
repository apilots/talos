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
    CacheControl, ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall,
    LlmProvider, ModelBlockCompleted, ModelBlockDelta, ModelBlockKind, ModelBlockStarted,
    ModelBlockStream, ModelBlockStreamEvent, ModelToolCallStart, NativeThinkingSupport,
    PromptCacheSupport, Role, SseLineDecoder, ToolChoice, ToolDefinition, Usage,
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

#[derive(Debug, Default)]
struct BailianModelBlockStreamState {
    reasoning_started: HashMap<usize, bool>,
    assistant_started: HashMap<usize, bool>,
    tool_started: HashMap<usize, bool>,
}

#[derive(Debug, Default)]
struct BailianModelToolAcc {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BailianModelCapability {
    Basic,
    Thinking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BailianMessage {
    role: String,
    content: Option<BailianMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<BailianToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum BailianMessageContent {
    Text(String),
    Blocks(Vec<BailianContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct BailianContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BailianTool {
    r#type: String,
    function: BailianFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BailianFunctionDefinition {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BailianToolCall {
    id: String,
    r#type: String,
    function: BailianFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BailianFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(crate) enum BailianToolChoice {
    Simple(String),
    Named {
        r#type: String,
        function: BailianToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BailianToolChoiceFunction {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct BailianRequest {
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
pub(crate) struct BailianStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BailianResponse {
    model: String,
    choices: Vec<BailianChoice>,
    #[serde(default)]
    usage: Option<BailianUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BailianChoice {
    message: BailianResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BailianResponseMessage {
    #[serde(default)]
    content: Option<BailianMessageContent>,
    #[serde(default)]
    tool_calls: Option<Vec<BailianToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BailianUsage {
    pub(crate) prompt_tokens: u32,
    pub(crate) completion_tokens: u32,
    pub(crate) total_tokens: u32,
    #[serde(default)]
    pub(crate) prompt_tokens_details: Option<BailianPromptTokensDetails>,
    #[serde(default)]
    pub(crate) cache_creation: Option<BailianCacheCreation>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BailianPromptTokensDetails {
    // DashScope OpenAI-compatible responses expose cache hits as `cached_tokens`.
    #[serde(default)]
    pub(crate) cached_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BailianCacheCreation {
    // DashScope OpenAI-compatible responses expose explicit cache writes here.
    #[serde(default)]
    pub(crate) cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub(crate) ephemeral_5m_input_tokens: Option<u32>,
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

    pub(crate) fn build_headers(&self) -> header::HeaderMap {
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

    pub(crate) fn convert_tool_choice(choice: Option<ToolChoice>) -> Option<BailianToolChoice> {
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

    pub(crate) fn convert_tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<BailianTool>> {
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

    pub(crate) fn convert_messages(messages: Vec<ChatMessage>) -> Vec<BailianMessage> {
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

                let content = if let Some(cache_control) = msg.cache_control {
                    Some(BailianMessageContent::Blocks(vec![BailianContentBlock {
                        block_type: "text".to_string(),
                        text: msg.content,
                        cache_control: Some(cache_control),
                    }]))
                } else {
                    Some(BailianMessageContent::Text(msg.content))
                };

                BailianMessage {
                    role,
                    content,
                    name: msg.name,
                    tool_call_id: msg.tool_call_id,
                    tool_calls,
                }
            })
            .collect()
    }

    pub(crate) fn parse_message_content(
        content: Option<BailianMessageContent>,
    ) -> TalosResult<String> {
        match content {
            Some(BailianMessageContent::Text(text)) => Ok(text),
            Some(BailianMessageContent::Blocks(blocks)) => {
                let mut text = String::new();
                for block in blocks {
                    if block.block_type != "text" {
                        return Err(TalosError::ValidationError(format!(
                            "Bailian response contains unsupported content block type: {}",
                            block.block_type
                        )));
                    }
                    text.push_str(&block.text);
                }
                Ok(text)
            }
            None => Ok(String::new()),
        }
    }

    pub(crate) fn parse_cache_creation_input_tokens(
        creation: &BailianCacheCreation,
    ) -> TalosResult<Option<u32>> {
        match (
            creation.cache_creation_input_tokens,
            creation.ephemeral_5m_input_tokens,
        ) {
            (Some(cache_creation_input_tokens), Some(ephemeral_5m_input_tokens))
                if cache_creation_input_tokens != ephemeral_5m_input_tokens =>
            {
                Err(TalosError::ValidationError(format!(
                    "Bailian cache creation token fields disagree: cache_creation_input_tokens={} ephemeral_5m_input_tokens={}",
                    cache_creation_input_tokens, ephemeral_5m_input_tokens
                )))
            }
            (Some(cache_creation_input_tokens), _) => Ok(Some(cache_creation_input_tokens)),
            (None, Some(ephemeral_5m_input_tokens)) => Ok(Some(ephemeral_5m_input_tokens)),
            (None, None) => Ok(None),
        }
    }

    pub(crate) fn usage_from_bailian(usage: BailianUsage) -> TalosResult<Usage> {
        Ok(Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cache_creation_input_tokens: match usage.cache_creation.as_ref() {
                Some(creation) => Self::parse_cache_creation_input_tokens(creation)?,
                None => None,
            },
            cache_read_input_tokens: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
        })
    }

    pub(crate) fn parse_tool_calls(
        tool_calls: Option<Vec<BailianToolCall>>,
    ) -> Option<Vec<FunctionCall>> {
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

    pub(crate) fn model_capability(model: &str) -> BailianModelCapability {
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

    pub(crate) fn native_thinking_config(request: &ChatRequest) -> Option<BailianThinkingConfig> {
        request
            .native_thinking
            .as_ref()
            .map(|config| BailianThinkingConfig {
                mode: BailianThinkingMode::Enabled,
                budget_tokens: config.budget_tokens,
            })
    }

    pub(crate) fn extract_reasoning_delta(delta: &Value) -> Option<String> {
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

    fn choice_index(choice: Option<&Value>) -> usize {
        choice
            .and_then(|c| c.get("index"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize
    }

    fn reasoning_model_item_id(choice_index: usize) -> String {
        format!("bailian-choice-{choice_index}-reasoning")
    }

    fn assistant_model_item_id(choice_index: usize) -> String {
        format!("bailian-choice-{choice_index}-assistant")
    }

    fn tool_model_item_id(index: usize) -> String {
        format!("bailian-tool-{index}")
    }

    async fn send_model_block_event(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        event: ModelBlockStreamEvent,
    ) -> bool {
        tx.send(Ok(event)).await.is_ok()
    }

    async fn ensure_reasoning_model_block(
        tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
        state: &mut BailianModelBlockStreamState,
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
        state: &mut BailianModelBlockStreamState,
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
        state: &mut BailianModelBlockStreamState,
        index: usize,
        acc: &BailianModelToolAcc,
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
        state: &mut BailianModelBlockStreamState,
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

    pub(crate) fn build_request(request: ChatRequest, stream: bool) -> BailianRequest {
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
            prompt_tokens_details: None,
            cache_creation: None,
        });

        Ok(ChatResponse {
            content: Self::parse_message_content(choice.message.content)?,
            reasoning: Vec::new(),
            model: bailian_resp.model,
            usage: Self::usage_from_bailian(usage)?,
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

    fn default_model(&self) -> String {
        "qwen-plus".to_string()
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        PromptCacheSupport::RequestLevel
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
                                            "Bailian stream invalid SSE JSON: {error}"
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
            let mut state = BailianModelBlockStreamState::default();
            let mut tool_acc: HashMap<usize, BailianModelToolAcc> = HashMap::new();
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
                                            "Bailian model block stream invalid SSE JSON: {error}"
                                        ))))
                                        .await;
                                    return;
                                }
                            };

                            if let Some(usage) = event.get("usage").and_then(|value| {
                                serde_json::from_value::<BailianUsage>(value.clone()).ok()
                            }) {
                                match Self::usage_from_bailian(usage) {
                                    Ok(mapped_usage) => final_usage = Some(mapped_usage),
                                    Err(err) => {
                                        let _ = tx.send(Err(err)).await;
                                        return;
                                    }
                                }
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
