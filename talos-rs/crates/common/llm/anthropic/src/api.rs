//! Talos LLM Anthropic provider.

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    CacheControl, ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall,
    LlmProvider, NativeThinkingConfig, NativeThinkingSupport, PromptCacheSupport, Role, ToolChoice,
    ToolDefinition, ToolStream, ToolStreamEvent, Usage,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// Anthropic API configuration.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub default_model: Option<String>,
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            timeout_secs: 120,
            default_model: None,
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

    pub fn with_default_model(mut self, model: impl Into<String>) -> Self {
        self.default_model = Some(model.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AnthropicMessage {
    role: String,
    content: Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AnthropicTool {
    name: String,
    description: String,
    #[serde(rename = "input_schema")]
    input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<AnthropicTextBlock>>,
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
pub(crate) struct AnthropicThinkingConfig {
    r#type: String,
    budget_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AnthropicResponse {
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
pub(crate) struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct AnthropicTextBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnthropicEndpointKind {
    Official,
    AliyunCodingPlan,
    Compatible,
}

/// Anthropic LLM provider.
pub struct AnthropicProvider {
    config: AnthropicConfig,
    client: Client,
}

impl AnthropicProvider {
    pub fn new(config: AnthropicConfig) -> Self {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .referer(false);

        if Self::endpoint_kind_for_base_url(&config.base_url)
            == AnthropicEndpointKind::AliyunCodingPlan
        {
            builder = builder.no_gzip().no_brotli().no_zstd().no_deflate();
        }

        let client = builder.build().expect("Failed to build HTTP client");

        Self { config, client }
    }

    fn endpoint_kind_for_base_url(base_url: &str) -> AnthropicEndpointKind {
        let base_url = base_url.trim_end_matches('/');
        if base_url.contains("api.anthropic.com") {
            AnthropicEndpointKind::Official
        } else if base_url.contains("coding.dashscope.aliyuncs.com/apps/anthropic")
            || base_url.contains("coding-intl.dashscope.aliyuncs.com/apps/anthropic")
        {
            AnthropicEndpointKind::AliyunCodingPlan
        } else {
            AnthropicEndpointKind::Compatible
        }
    }

    fn endpoint_kind(&self) -> AnthropicEndpointKind {
        Self::endpoint_kind_for_base_url(&self.config.base_url)
    }

    fn claude_code_user_agent() -> String {
        let entrypoint = env::var("CLAUDE_CODE_ENTRYPOINT").unwrap_or_else(|_| "cli".to_string());
        let user_type = env::var("USER_TYPE").unwrap_or_else(|_| "external".to_string());
        let agent_sdk_version = env::var("CLAUDE_AGENT_SDK_VERSION")
            .ok()
            .map(|value| format!(", agent-sdk/{}", value))
            .unwrap_or_default();
        let client_app = env::var("CLAUDE_AGENT_SDK_CLIENT_APP")
            .ok()
            .map(|value| format!(", client-app/{}", value))
            .unwrap_or_default();

        format!(
            "claude-cli/{} ({}, {}{}{})",
            env!("CARGO_PKG_VERSION"),
            user_type,
            entrypoint,
            agent_sdk_version,
            client_app
        )
    }

    pub(crate) fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));

        match self.endpoint_kind() {
            AnthropicEndpointKind::Official => {
                headers.insert(
                    "x-api-key",
                    header::HeaderValue::from_str(&self.config.api_key).unwrap(),
                );
                headers.insert(
                    "anthropic-version",
                    header::HeaderValue::from_static("2023-06-01"),
                );
            }
            AnthropicEndpointKind::AliyunCodingPlan => {
                headers.insert(
                    header::AUTHORIZATION,
                    header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
                        .unwrap(),
                );
                headers.insert(
                    "anthropic-beta",
                    header::HeaderValue::from_static("claude-code-20250219"),
                );
                headers.insert("x-app", header::HeaderValue::from_static("cli"));
                headers.insert(
                    header::USER_AGENT,
                    header::HeaderValue::from_str(&Self::claude_code_user_agent()).unwrap(),
                );
            }
            AnthropicEndpointKind::Compatible => {
                headers.insert(
                    header::AUTHORIZATION,
                    header::HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
                        .unwrap(),
                );
            }
        }

        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    pub(crate) fn text_block(text: String, cache_control: Option<CacheControl>) -> Value {
        serde_json::to_value(AnthropicTextBlock {
            block_type: "text".to_string(),
            text,
            cache_control,
        })
        .expect("anthropic text block should serialize")
    }

    pub(crate) fn convert_tool_choice(choice: Option<ToolChoice>) -> Option<AnthropicToolChoice> {
        match choice {
            Some(ToolChoice::Auto) => Some(AnthropicToolChoice::Auto),
            Some(ToolChoice::Required) => Some(AnthropicToolChoice::Any),
            Some(ToolChoice::Named { name }) => Some(AnthropicToolChoice::Tool { name }),
            Some(ToolChoice::None) | None => None,
        }
    }

    pub(crate) fn convert_tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<AnthropicTool>> {
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

    pub(crate) fn convert_messages(
        messages: Vec<ChatMessage>,
    ) -> (Option<Vec<AnthropicTextBlock>>, Vec<AnthropicMessage>) {
        let mut system_blocks: Option<Vec<AnthropicTextBlock>> = None;
        let mut anthropic_messages = Vec::new();

        for msg in messages {
            match msg.role {
                Role::System => {
                    system_blocks
                        .get_or_insert_with(Vec::new)
                        .push(AnthropicTextBlock {
                            block_type: "text".to_string(),
                            text: msg.content,
                            cache_control: msg.cache_control,
                        });
                }
                Role::User => {
                    anthropic_messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: Value::Array(vec![Self::text_block(
                            msg.content,
                            msg.cache_control,
                        )]),
                    });
                }
                Role::Assistant => {
                    if let Some(tool_calls) = msg.tool_calls {
                        let mut blocks = Vec::new();
                        if !msg.content.is_empty() {
                            blocks.push(Self::text_block(msg.content, msg.cache_control));
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
                            content: Value::Array(vec![Self::text_block(
                                msg.content,
                                msg.cache_control,
                            )]),
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

        (system_blocks, anthropic_messages)
    }

    pub(crate) fn parse_response_content(
        content: &[AnthropicContent],
    ) -> (String, Vec<String>, Option<Vec<FunctionCall>>) {
        let mut text = Vec::new();
        let mut reasoning = Vec::new();
        let mut calls = Vec::new();

        for block in content {
            match block.content_type.as_str() {
                "text" => {
                    if let Some(fragment) = block.text.as_ref() {
                        text.push(fragment.clone());
                    }
                }
                "thinking" => {
                    if let Some(fragment) = block.thinking.as_ref() {
                        reasoning.push(fragment.clone());
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
        (text.join(""), reasoning, tool_calls)
    }

    pub(crate) fn build_request(request: ChatRequest, stream: Option<bool>) -> AnthropicRequest {
        let (system, messages) = Self::convert_messages(request.messages);
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
            stream,
        }
    }

    pub(crate) fn convert_native_thinking(
        config: &NativeThinkingConfig,
    ) -> AnthropicThinkingConfig {
        AnthropicThinkingConfig {
            r#type: "enabled".to_string(),
            budget_tokens: config.budget_tokens.unwrap_or(2048),
        }
    }

    pub(crate) fn supports_native_thinking_model(model: &str) -> bool {
        let normalized = model.trim().to_ascii_lowercase();
        normalized.starts_with("claude-3-7-sonnet")
            || normalized.starts_with("claude-sonnet-4")
            || normalized.starts_with("claude-opus-4")
            || normalized.starts_with("claude-4")
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let anthropic_req = Self::build_request(request, None);
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

        let (content, reasoning, tool_calls) =
            Self::parse_response_content(&anthropic_resp.content);

        Ok(ChatResponse {
            content,
            reasoning,
            model: anthropic_resp.model,
            usage: Usage {
                prompt_tokens: anthropic_resp.usage.input_tokens,
                completion_tokens: anthropic_resp.usage.output_tokens,
                total_tokens: anthropic_resp.usage.input_tokens
                    + anthropic_resp.usage.output_tokens,
                cache_creation_input_tokens: anthropic_resp.usage.cache_creation_input_tokens,
                cache_read_input_tokens: anthropic_resp.usage.cache_read_input_tokens,
            },
            finish_reason: anthropic_resp.stop_reason,
            tool_calls,
        })
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let anthropic_req = Self::build_request(request, Some(true));

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
        self.config
            .default_model
            .as_deref()
            .unwrap_or("claude-3-5-sonnet-20241022")
    }

    fn native_thinking_support(&self, model: &str) -> NativeThinkingSupport {
        if Self::supports_native_thinking_model(model) {
            NativeThinkingSupport::Supported
        } else {
            NativeThinkingSupport::Unsupported
        }
    }

    fn prompt_cache_support(&self) -> PromptCacheSupport {
        PromptCacheSupport::RequestLevel
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
            let mut stop_reason: Option<String> = None;
            let mut line_buf = String::new();
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        for line in drain_sse_lines(&mut line_buf, &bytes) {
                            let Some(data) = line.strip_prefix("data:") else {
                                continue;
                            };
                            let data = data.trim_start();
                            let Ok(event) = serde_json::from_str::<Value>(data) else {
                                continue;
                            };

                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match event_type {
                                "error" => {
                                    let msg = event
                                        .get("error")
                                        .and_then(|e| e.get("message"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown error");
                                    let _ = tx
                                        .send(Err(TalosError::InternalError(format!(
                                            "Anthropic stream error: {}",
                                            msg
                                        ))))
                                        .await;
                                    return;
                                }
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
                                "message_delta" => {
                                    if let Some(reason) = event
                                        .get("delta")
                                        .and_then(|d| d.get("stop_reason"))
                                        .and_then(|v| v.as_str())
                                    {
                                        stop_reason = Some(reason.to_string());
                                    }
                                }
                                "message_stop" => {
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: String::new(),
                                            finish_reason: Some(
                                                stop_reason
                                                    .take()
                                                    .unwrap_or_else(|| "stop".to_string()),
                                            ),
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
            let mut stop_reason: Option<String> = None;
            let mut output_tokens: Option<u32> = None;
            let mut line_buf = String::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        for line in drain_sse_lines(&mut line_buf, &bytes) {
                            let Some(data) = line.strip_prefix("data:") else {
                                continue;
                            };
                            let data = data.trim_start();
                            let Ok(event) = serde_json::from_str::<Value>(data) else {
                                continue;
                            };

                            let event_type =
                                event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                            match event_type {
                                "error" => {
                                    let msg = event
                                        .get("error")
                                        .and_then(|e| e.get("message"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown error");
                                    let _ = tx
                                        .send(Err(TalosError::InternalError(format!(
                                            "Anthropic stream error: {}",
                                            msg
                                        ))))
                                        .await;
                                    return;
                                }
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
                                    let delta = event.get("delta");

                                    let delta_type = delta
                                        .and_then(|d| d.get("type"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    match delta_type {
                                        "thinking_delta" => {
                                            if let Some(thinking) = delta
                                                .and_then(|d| d.get("thinking"))
                                                .and_then(|v| v.as_str())
                                            {
                                                debug!(
                                                    "stream_with_tools: thinking_delta len={}",
                                                    thinking.len()
                                                );
                                                if tx
                                                    .send(Ok(ToolStreamEvent::ReasoningDelta {
                                                        delta: thinking.to_string(),
                                                    }))
                                                    .await
                                                    .is_err()
                                                {
                                                    return;
                                                }
                                            }
                                        }
                                        "signature_delta" => {
                                            if let Some(sig) = delta
                                                .and_then(|d| d.get("signature"))
                                                .and_then(|v| v.as_str())
                                            {
                                                if tx
                                                    .send(Ok(ToolStreamEvent::ReasoningSignature {
                                                        signature: sig.to_string(),
                                                    }))
                                                    .await
                                                    .is_err()
                                                {
                                                    return;
                                                }
                                            }
                                        }
                                        "text_delta" => {
                                            if let Some(text) = delta
                                                .and_then(|d| d.get("text"))
                                                .and_then(|v| v.as_str())
                                            {
                                                if tx
                                                    .send(Ok(ToolStreamEvent::TextDelta {
                                                        delta: text.to_string(),
                                                    }))
                                                    .await
                                                    .is_err()
                                                {
                                                    return;
                                                }
                                            }
                                        }
                                        "input_json_delta" => {
                                            if let Some(partial_json) = delta
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
                                        _ => {}
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
                                "message_delta" => {
                                    if let Some(reason) = event
                                        .get("delta")
                                        .and_then(|d| d.get("stop_reason"))
                                        .and_then(|v| v.as_str())
                                    {
                                        stop_reason = Some(reason.to_string());
                                    }
                                    if let Some(tokens) = event
                                        .get("usage")
                                        .and_then(|u| u.get("output_tokens"))
                                        .and_then(|v| v.as_u64())
                                    {
                                        output_tokens = Some(tokens as u32);
                                    }
                                }
                                "message_stop" => {
                                    let usage = output_tokens.map(|out| Usage {
                                        prompt_tokens: 0,
                                        completion_tokens: out,
                                        total_tokens: out,
                                        cache_creation_input_tokens: None,
                                        cache_read_input_tokens: None,
                                    });
                                    let _ = tx
                                        .send(Ok(ToolStreamEvent::Done {
                                            finish_reason: stop_reason
                                                .take()
                                                .or_else(|| Some("stop".to_string())),
                                            usage,
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

/// Accumulates raw SSE bytes into `buf` and returns all complete lines,
/// retaining any trailing incomplete fragment in `buf` for the next call.
fn drain_sse_lines(buf: &mut String, bytes: &[u8]) -> Vec<String> {
    buf.push_str(&String::from_utf8_lossy(bytes));
    let mut lines = Vec::new();
    while let Some(pos) = buf.find('\n') {
        let line = buf[..pos].trim_end_matches('\r').to_string();
        buf.drain(..=pos);
        lines.push(line);
    }
    lines
}
