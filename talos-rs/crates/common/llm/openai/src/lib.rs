//! Talos LLM OpenAI provider.

use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    protocols::{chat as chat_protocol, responses as responses_protocol},
    ChatRequest, ChatResponse, ChatStream, LlmProvider, ModelBlockStream, ReasoningEffort,
    ReasoningSupport,
};
use tracing::{debug, error, info};

/// OpenAI wire protocol selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAIProtocol {
    Chat,
    Responses,
}

/// OpenAI API configuration.
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: String,
    pub timeout_secs: u64,
    pub provider_name: String,
    pub display_name: String,
    pub item_prefix: String,
    pub default_model: String,
    pub protocol: OpenAIProtocol,
}

impl OpenAIConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            timeout_secs: 60,
            provider_name: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            item_prefix: "openai".to_string(),
            default_model: "gpt-5.5".to_string(),
            protocol: OpenAIProtocol::Chat,
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

    pub fn with_identity(
        mut self,
        provider_name: impl Into<String>,
        display_name: impl Into<String>,
        default_model: impl Into<String>,
    ) -> Self {
        self.provider_name = provider_name.into();
        self.display_name = display_name.into();
        self.item_prefix = self.provider_name.clone();
        self.default_model = default_model.into();
        self
    }

    pub fn with_protocol(mut self, protocol: OpenAIProtocol) -> Self {
        self.protocol = protocol;
        self
    }
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

    fn insert_header_value(
        headers: &mut header::HeaderMap,
        name: &'static str,
        value: impl AsRef<str>,
    ) {
        if let Ok(header_value) = header::HeaderValue::from_str(value.as_ref()) {
            headers.insert(name, header_value);
        }
    }

    fn insert_talos_headers(headers: &mut header::HeaderMap) {
        headers.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
        headers.insert("x-talos-client", header::HeaderValue::from_static("talos"));
        headers.insert(
            "x-talos-version",
            header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        );
        Self::insert_header_value(
            headers,
            header::USER_AGENT.as_str(),
            format!("talos/{}", env!("CARGO_PKG_VERSION")),
        );
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
        Self::insert_talos_headers(&mut headers);

        headers
    }

    fn request_options(request: &ChatRequest) -> chat_protocol::ChatCompletionRequestOptions {
        chat_protocol::ChatCompletionRequestOptions {
            reasoning_effort: request.reasoning_effort.map(Self::convert_reasoning_effort),
            ..Default::default()
        }
    }

    fn build_request(request: ChatRequest, stream: bool) -> chat_protocol::ChatCompletionRequest {
        let options = Self::request_options(&request);
        chat_protocol::request(request, stream, options)
    }

    fn responses_request(
        request: ChatRequest,
        stream: Option<bool>,
    ) -> responses_protocol::ResponsesRequest {
        responses_protocol::request(request, stream)
    }

    fn endpoint_url(&self, endpoint: &str) -> String {
        let base_url = self.config.base_url.trim_end_matches('/');
        let endpoint = endpoint.trim_start_matches('/');
        format!("{base_url}/{endpoint}")
    }

    fn convert_reasoning_effort(effort: ReasoningEffort) -> String {
        chat_protocol::reasoning_effort(effort)
    }

    fn supports_reasoning_model(model: &str) -> bool {
        let normalized = model.trim().to_ascii_lowercase();
        normalized.starts_with("o1")
            || normalized.starts_with("o3")
            || normalized.starts_with("o4")
            || normalized.starts_with("gpt-5")
    }

    async fn send_chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        match self.config.protocol {
            OpenAIProtocol::Chat => self.send_chat_completion(request).await,
            OpenAIProtocol::Responses => self.send_responses(request).await,
        }
    }

    async fn send_chat_completion(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let openai_req = Self::build_request(request, false);

        let url = self.endpoint_url("chat/completions");
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
            error!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            );
            return Err(TalosError::InternalError(format!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            )));
        }

        let openai_resp: chat_protocol::ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        chat_protocol::chat_response(openai_resp, &self.config.display_name)
    }

    async fn send_responses(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let openai_req = Self::responses_request(request, None);

        let url = self.endpoint_url("responses");
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
            error!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            );
            return Err(TalosError::InternalError(format!(
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            )));
        }

        let openai_resp: responses_protocol::ResponsesResponse = response
            .json()
            .await
            .map_err(|e| TalosError::InternalError(format!("Failed to parse response: {}", e)))?;

        Ok(responses_protocol::chat_response(openai_resp))
    }

    async fn send_stream(&self, request: ChatRequest) -> TalosResult<reqwest::Response> {
        let (url, body) = match self.config.protocol {
            OpenAIProtocol::Chat => (
                self.endpoint_url("chat/completions"),
                serde_json::to_value(Self::build_request(request, true)).map_err(|e| {
                    TalosError::InternalError(format!("Failed to serialize request: {}", e))
                })?,
            ),
            OpenAIProtocol::Responses => (
                self.endpoint_url("responses"),
                serde_json::to_value(Self::responses_request(request, Some(true))).map_err(
                    |e| TalosError::InternalError(format!("Failed to serialize request: {}", e)),
                )?,
            ),
        };
        let headers = self.build_headers();

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
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
                "{} API error: {} - {}",
                self.config.display_name, status, error_text
            )));
        }

        Ok(response)
    }
}

#[async_trait]
impl LlmProvider for OpenAIProvider {
    fn name(&self) -> &str {
        &self.config.provider_name
    }

    fn default_model(&self) -> String {
        self.config.default_model.clone()
    }

    fn reasoning_support(&self, model: &str) -> ReasoningSupport {
        if Self::supports_reasoning_model(model) {
            ReasoningSupport::Supported
        } else {
            ReasoningSupport::Unsupported
        }
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!("Sending chat request to {} API", self.config.display_name);
        self.send_chat(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        info!(
            "Sending tool-aware chat request to {} API",
            self.config.display_name
        );
        self.send_chat(request).await
    }

    async fn stream(&self, request: ChatRequest) -> TalosResult<ChatStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenAIProtocol::Chat => Ok(chat_protocol::stream(
                response,
                self.config.display_name.clone(),
            )),
            OpenAIProtocol::Responses => Ok(responses_protocol::stream(
                response,
                self.config.display_name.clone(),
            )),
        }
    }

    async fn stream_model_blocks(&self, request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let response = self.send_stream(request).await?;
        match self.config.protocol {
            OpenAIProtocol::Chat => Ok(chat_protocol::model_blocks(
                response,
                chat_protocol::ChatCompletionModelBlocks::openai().with_identity(
                    self.config.display_name.clone(),
                    self.config.item_prefix.clone(),
                ),
            )),
            OpenAIProtocol::Responses => Ok(responses_protocol::model_blocks(
                response,
                responses_protocol::ResponsesModelBlocks::openai().with_identity(
                    self.config.display_name.clone(),
                    self.config.item_prefix.clone(),
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use talos_llm_provider::{
        CacheControl, CacheControlKind, ChatMessage, ModelBlockDelta, ModelBlockKind,
        ModelBlockStreamEvent, PromptCacheSupport, ReasoningEffort, ToolDefinition,
    };
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
        assert_eq!(config.protocol, OpenAIProtocol::Chat);
    }

    #[test]
    fn test_openai_config_can_select_responses_protocol() {
        let config = OpenAIConfig::new("test-api-key").with_protocol(OpenAIProtocol::Responses);

        assert_eq!(config.protocol, OpenAIProtocol::Responses);
    }

    #[test]
    fn test_endpoint_url_appends_protocol_endpoint_to_base_url() {
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-api-key")
                .with_base_url("https://dashscope.aliyuncs.com/compatible-mode/v1/"),
        );

        assert_eq!(
            provider.endpoint_url("chat/completions"),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
        assert_eq!(
            provider.endpoint_url("responses"),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/responses"
        );
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
    async fn test_stream_model_blocks_maps_responses_events() {
        let body = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\"}}\n\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"output_index\":0,\"delta\":\"think\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"delta\":\"hello\"}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\"{\\\"path\\\"\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"delta\":\":\\\"/tmp/a\\\"}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"read\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"
        )
        .to_string();
        let base_url = spawn_sse_server(body).await;
        let provider = OpenAIProvider::new(
            OpenAIConfig::new("test-api-key")
                .with_base_url(base_url)
                .with_protocol(OpenAIProtocol::Responses),
        );
        let stream = provider
            .stream_model_blocks(ChatRequest::new_user("test", "gpt-test"))
            .await
            .expect("stream should start");

        let events = collect_model_events(stream).await;

        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockStarted(started)
                if started.model_item_id == "openai-response-0-reasoning"
                    && started.provider_item_id.as_deref() == Some("rs_1")
                    && started.kind == ModelBlockKind::Reasoning
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::ReasoningText { delta } }
                if model_item_id == "openai-response-0-reasoning" && delta == "think"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::AssistantText { delta } }
                if model_item_id == "openai-response-1-text" && delta == "hello"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockStarted(started)
                if started.model_item_id == "openai-response-2-tool"
                    && started.provider_item_id.as_deref() == Some("fc_1")
                    && started.tool_call.as_ref().and_then(|call| call.call_id.as_deref()) == Some("call_1")
                    && started.tool_call.as_ref().and_then(|call| call.tool_name.as_deref()) == Some("read")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockDelta { model_item_id, delta: ModelBlockDelta::ToolArguments { delta } }
                if model_item_id == "openai-response-2-tool" && delta == "{\"path\""
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::BlockCompleted(completed)
                if completed.model_item_id == "openai-response-2-tool"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelBlockStreamEvent::Done { usage: Some(usage), finish_reason: Some(reason) }
                if usage.total_tokens == 3 && reason == "stop"
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
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are a helpful assistant"),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
        ];

        let openai_msgs = chat_protocol::messages(messages, false);

        assert_eq!(openai_msgs.len(), 3);
        assert_eq!(openai_msgs[0].role, "system");
        assert!(matches!(
            openai_msgs[0].content.as_ref(),
            Some(chat_protocol::ChatCompletionMessageContent::Text(text))
                if text == "You are a helpful assistant"
        ));
        assert_eq!(openai_msgs[1].role, "user");
        assert!(matches!(
            openai_msgs[1].content.as_ref(),
            Some(chat_protocol::ChatCompletionMessageContent::Text(text)) if text == "Hello"
        ));
        assert_eq!(openai_msgs[2].role, "assistant");
        assert!(matches!(
            openai_msgs[2].content.as_ref(),
            Some(chat_protocol::ChatCompletionMessageContent::Text(text)) if text == "Hi there!"
        ));
    }

    #[test]
    fn test_openai_request_serialization_with_tools() {
        let req = chat_protocol::request(
            ChatRequest::builder()
                .model("gpt-4o")
                .messages(vec![ChatMessage::user("Hello")])
                .temperature(0.7)
                .max_tokens(100)
                .tools(vec![ToolDefinition::new(
                    "read",
                    "Read file",
                    Some(serde_json::json!({"type": "object"})),
                )])
                .tool_choice(talos_llm_provider::ToolChoice::Auto)
                .parallel_tool_calls(true)
                .build(),
            false,
            chat_protocol::ChatCompletionRequestOptions::default(),
        );

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o\""));
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"tool_choice\":\"auto\""));
    }

    #[test]
    fn test_build_request_maps_reasoning_effort() {
        let request = ChatRequest::builder()
            .model("gpt-4o")
            .messages(vec![ChatMessage::user("hello")])
            .reasoning_effort(ReasoningEffort::High)
            .build();

        let built = OpenAIProvider::build_request(request, false);

        assert_eq!(built.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn test_parse_tool_calls() {
        let calls = vec![chat_protocol::ChatCompletionToolCall {
            id: "call_1".to_string(),
            r#type: "function".to_string(),
            function: chat_protocol::ChatCompletionFunctionCall {
                name: "read".to_string(),
                arguments: "{\"path\":\"/tmp/a\"}".to_string(),
            },
        }];

        let parsed = chat_protocol::tool_calls(Some(calls)).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "read");
        assert_eq!(parsed[0].arguments["path"], "/tmp/a");
    }

    #[test]
    fn test_provider_name() {
        let config = OpenAIConfig::new("test-key");
        let provider = OpenAIProvider::new(config);
        assert_eq!(provider.name(), "openai");
        assert_eq!(provider.default_model(), "gpt-5.5");
        assert_eq!(
            provider.prompt_cache_support(),
            PromptCacheSupport::Unsupported
        );
    }

    #[test]
    fn test_provider_identity_can_be_overridden() {
        let config = OpenAIConfig::new("test-key").with_identity(
            "openrouter",
            "OpenRouter",
            "anthropic/claude-sonnet-4.5",
        );
        let provider = OpenAIProvider::new(config);

        assert_eq!(provider.name(), "openrouter");
        assert_eq!(provider.default_model(), "anthropic/claude-sonnet-4.5");
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
    fn test_reasoning_support_is_model_gated() {
        let provider = OpenAIProvider::new(OpenAIConfig::new("test-key"));

        assert_eq!(
            provider.reasoning_support("o3-mini"),
            ReasoningSupport::Supported
        );
        assert_eq!(
            provider.reasoning_support("gpt-5"),
            ReasoningSupport::Supported
        );
        assert_eq!(
            provider.reasoning_support("gpt-4o"),
            ReasoningSupport::Unsupported
        );
    }

    #[test]
    fn test_extract_reasoning_delta_ignores_bailian_reasoning_content() {
        let delta = serde_json::json!({
            "reasoning_content": "bailian-only"
        });
        assert!(chat_protocol::reasoning_delta(&delta, &["reasoning"]).is_none());
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
        assert_eq!(
            headers.get("accept").and_then(|value| value.to_str().ok()),
            Some("*/*")
        );
        assert_eq!(
            headers
                .get("x-talos-client")
                .and_then(|value| value.to_str().ok()),
            Some("talos")
        );
        assert_eq!(
            headers
                .get("x-talos-version")
                .and_then(|value| value.to_str().ok()),
            Some(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some(concat!("talos/", env!("CARGO_PKG_VERSION")))
        );
    }

    #[test]
    fn test_convert_tools() {
        let defs = vec![ToolDefinition::new(
            "read",
            "Read file",
            Some(serde_json::json!({"type":"object"})),
        )];
        let converted = chat_protocol::tools(Some(defs)).unwrap();
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].function.name, "read");
    }
}
