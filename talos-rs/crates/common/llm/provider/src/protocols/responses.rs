use crate::{
    ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall,
    ModelBlockCompleted, ModelBlockDelta, ModelBlockKind, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, ModelToolCallStart, ReasoningEffort, Role, SseLineDecoder, ToolChoice,
    ToolDefinition, Usage,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ResponsesToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponsesReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ResponsesToolChoice {
    Simple(String),
    Named { r#type: String, name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesReasoning {
    pub effort: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub output_text: Option<String>,
    #[serde(default)]
    pub output: Vec<Value>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    #[serde(default)]
    pub input_tokens_details: Option<ResponsesInputTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesInputTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ResponsesModelBlocks {
    pub provider_name: String,
    pub item_prefix: String,
}

impl ResponsesModelBlocks {
    pub fn openai() -> Self {
        Self {
            provider_name: "OpenAI".to_string(),
            item_prefix: "openai".to_string(),
        }
    }

    pub fn with_identity(
        mut self,
        provider_name: impl Into<String>,
        item_prefix: impl Into<String>,
    ) -> Self {
        self.provider_name = provider_name.into();
        self.item_prefix = item_prefix.into();
        self
    }
}

#[derive(Debug, Clone)]
enum ActiveResponsesBlock {
    Reasoning { model_item_id: String },
    AssistantText { model_item_id: String },
    ToolCall { model_item_id: String },
}

pub fn request(request: ChatRequest, stream: Option<bool>) -> ResponsesRequest {
    ResponsesRequest {
        model: request.model,
        input: input(request.messages),
        temperature: request.temperature,
        max_output_tokens: request.max_tokens,
        top_p: request.top_p,
        tools: tools(request.tools),
        tool_choice: tool_choice(request.tool_choice),
        parallel_tool_calls: request.parallel_tool_calls,
        reasoning: request.reasoning_effort.map(reasoning),
        stream,
    }
}

pub fn input(messages: Vec<ChatMessage>) -> Vec<Value> {
    let mut items = Vec::new();

    for message in messages {
        match message.role {
            Role::System | Role::User => {
                items.push(serde_json::json!({
                    "role": role_name(message.role),
                    "content": [{
                        "type": "input_text",
                        "text": message.content,
                    }],
                }));
            }
            Role::Assistant => {
                if !message.content.is_empty() {
                    items.push(serde_json::json!({
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": message.content,
                        }],
                    }));
                }
                if let Some(tool_calls) = message.tool_calls {
                    for call in tool_calls {
                        items.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": call.id,
                            "name": call.name,
                            "arguments": call.arguments.to_string(),
                        }));
                    }
                }
            }
            Role::Tool => {
                items.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": message.tool_call_id.unwrap_or_default(),
                    "output": message.content,
                }));
            }
        }
    }

    items
}

pub fn tools(definitions: Option<Vec<ToolDefinition>>) -> Option<Vec<ResponsesTool>> {
    definitions.map(|definitions| {
        definitions
            .into_iter()
            .map(|tool| ResponsesTool {
                tool_type: "function".to_string(),
                name: tool.name,
                description: tool.description,
                parameters: tool.parameters,
            })
            .collect()
    })
}

pub fn tool_choice(choice: Option<ToolChoice>) -> Option<ResponsesToolChoice> {
    match choice {
        Some(ToolChoice::Auto) => Some(ResponsesToolChoice::Simple("auto".to_string())),
        Some(ToolChoice::Required) => Some(ResponsesToolChoice::Simple("required".to_string())),
        Some(ToolChoice::None) => Some(ResponsesToolChoice::Simple("none".to_string())),
        Some(ToolChoice::Named { name }) => Some(ResponsesToolChoice::Named {
            r#type: "function".to_string(),
            name,
        }),
        None => None,
    }
}

pub fn reasoning(effort: ReasoningEffort) -> ResponsesReasoning {
    let effort = match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    };
    ResponsesReasoning {
        effort: effort.to_string(),
    }
}

pub fn usage(usage: Option<ResponsesUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    Usage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        cache_creation_input_tokens: None,
        cache_read_input_tokens: usage
            .input_tokens_details
            .and_then(|details| details.cached_tokens),
    }
}

pub fn chat_response(response: ResponsesResponse) -> ChatResponse {
    let (content, reasoning, tool_calls) = response_content(&response);
    ChatResponse {
        content,
        reasoning,
        model: response.model.unwrap_or_default(),
        usage: usage(response.usage),
        finish_reason: finish_reason(response.status.as_deref()),
        tool_calls,
    }
}

pub fn response_content(
    response: &ResponsesResponse,
) -> (String, Vec<String>, Option<Vec<FunctionCall>>) {
    let mut text = Vec::new();
    let mut reasoning = Vec::new();
    let mut calls = Vec::new();

    let has_output_text = response
        .output_text
        .as_ref()
        .is_some_and(|text| !text.is_empty());
    if let Some(output_text) = response.output_text.as_ref() {
        if !output_text.is_empty() {
            text.push(output_text.clone());
        }
    }

    for item in &response.output {
        match item_type(item) {
            "message" if !has_output_text => text.extend(text_from_message_item(item)),
            "reasoning" => reasoning.extend(reasoning_from_item(item)),
            "function_call" => calls.push(function_call(item)),
            _ => {}
        }
    }

    let tool_calls = if calls.is_empty() { None } else { Some(calls) };
    (text.join(""), reasoning, tool_calls)
}

pub fn stream(response: reqwest::Response, provider_name: impl Into<String>) -> ChatStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);
    let provider_name = provider_name.into();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut decoder = SseLineDecoder::new();

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
                                    finish_reason: Some("stop".to_string()),
                                }))
                                .await;
                            return;
                        }

                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err(TalosError::InternalError(format!(
                                        "{provider_name} responses stream invalid SSE JSON: {error}"
                                    ))))
                                    .await;
                                return;
                            }
                        };

                        if let Some(error) = stream_error(&event) {
                            let _ = tx
                                .send(Err(TalosError::InternalError(format!(
                                    "{provider_name} responses stream error: {error}"
                                ))))
                                .await;
                            return;
                        }

                        match event_type(&event) {
                            "response.output_text.delta" => {
                                if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                    let _ = tx
                                        .send(Ok(ChatChunk {
                                            delta: delta.to_string(),
                                            finish_reason: None,
                                        }))
                                        .await;
                                }
                            }
                            "response.completed" => {
                                let reason = event
                                    .get("response")
                                    .and_then(|r| r.get("status"))
                                    .and_then(|v| v.as_str())
                                    .and_then(|status| finish_reason(Some(status)))
                                    .or_else(|| Some("stop".to_string()));
                                let _ = tx
                                    .send(Ok(ChatChunk {
                                        delta: String::new(),
                                        finish_reason: reason,
                                    }))
                                    .await;
                                return;
                            }
                            _ => {}
                        }
                    }
                }
                Err(error) => {
                    let _ = tx
                        .send(Err(TalosError::InternalError(format!(
                            "Stream error: {error}"
                        ))))
                        .await;
                    return;
                }
            }
        }
    });

    ChatStream::new(rx)
}

pub fn model_blocks(
    response: reqwest::Response,
    options: ResponsesModelBlocks,
) -> ModelBlockStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(100);

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut decoder = SseLineDecoder::new();
        let mut active_blocks: HashMap<String, ActiveResponsesBlock> = HashMap::new();
        let mut final_usage: Option<Usage> = None;
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
                            if !complete_open_blocks(&tx, &mut active_blocks).await {
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
                                        "{} responses model block stream invalid SSE JSON: {error}",
                                        options.provider_name
                                    ))))
                                    .await;
                                return;
                            }
                        };

                        if let Some(error) = stream_error(&event) {
                            let _ = tx
                                .send(Err(TalosError::InternalError(format!(
                                    "{} responses stream error: {error}",
                                    options.provider_name
                                ))))
                                .await;
                            return;
                        }

                        match event_type(&event) {
                            "response.output_item.added" => {
                                if !start_output_item(
                                    &tx,
                                    &mut active_blocks,
                                    &event,
                                    &options.item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "response.output_text.delta" => {
                                if !send_text_delta(
                                    &tx,
                                    &mut active_blocks,
                                    &event,
                                    &options.item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "response.reasoning_summary_text.delta"
                            | "response.reasoning_text.delta" => {
                                if !send_reasoning_delta(
                                    &tx,
                                    &mut active_blocks,
                                    &event,
                                    &options.item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "response.function_call_arguments.delta" => {
                                if !send_tool_delta(
                                    &tx,
                                    &mut active_blocks,
                                    &event,
                                    &options.item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "response.output_item.done" => {
                                if !complete_output_item(&tx, &mut active_blocks, &event).await {
                                    return;
                                }
                            }
                            "response.completed" => {
                                final_usage = event
                                    .get("response")
                                    .and_then(|r| r.get("usage"))
                                    .and_then(|value| {
                                        serde_json::from_value::<ResponsesUsage>(value.clone()).ok()
                                    })
                                    .map(|usage_value| usage(Some(usage_value)));
                                final_finish_reason = event
                                    .get("response")
                                    .and_then(|r| r.get("status"))
                                    .and_then(|v| v.as_str())
                                    .and_then(|status| finish_reason(Some(status)));
                                if !complete_open_blocks(&tx, &mut active_blocks).await {
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
                            _ => {}
                        }
                    }
                }
                Err(error) => {
                    let _ = tx
                        .send(Err(TalosError::InternalError(format!(
                            "Stream error: {error}"
                        ))))
                        .await;
                    return;
                }
            }
        }
    });

    ModelBlockStream::new(rx)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn item_type(item: &Value) -> &str {
    item.get("type").and_then(|v| v.as_str()).unwrap_or("")
}

fn event_type(event: &Value) -> &str {
    event.get("type").and_then(|v| v.as_str()).unwrap_or("")
}

fn output_index(event: &Value) -> usize {
    event
        .get("output_index")
        .or_else(|| event.get("index"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

fn text_from_message_item(item: &Value) -> Vec<String> {
    let Some(content) = item.get("content").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| {
            let block_type = item_type(block);
            if block_type == "output_text" || block_type == "text" {
                block
                    .get("text")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            } else {
                None
            }
        })
        .collect()
}

fn reasoning_from_item(item: &Value) -> Vec<String> {
    let mut fragments = Vec::new();
    for key in ["summary", "content"] {
        if let Some(items) = item.get(key).and_then(|v| v.as_array()) {
            for block in items {
                if let Some(text) = block
                    .get("text")
                    .and_then(|v| v.as_str())
                    .or_else(|| block.get("summary_text").and_then(|v| v.as_str()))
                {
                    fragments.push(text.to_string());
                }
            }
        }
    }
    fragments
}

fn function_call(item: &Value) -> FunctionCall {
    let arguments = item
        .get("arguments")
        .and_then(|v| v.as_str())
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .or_else(|| item.get("arguments").cloned())
        .unwrap_or(Value::Null);

    FunctionCall::new(
        item.get("call_id")
            .or_else(|| item.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        item.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        arguments,
    )
}

fn finish_reason(status: Option<&str>) -> Option<String> {
    match status {
        Some("completed") => Some("stop".to_string()),
        Some(status) if !status.is_empty() => Some(status.to_string()),
        _ => None,
    }
}

fn stream_error(event: &Value) -> Option<String> {
    match event_type(event) {
        "error" | "response.failed" | "response.incomplete" => Some(
            event
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .or_else(|| {
                    event
                        .get("response")
                        .and_then(|r| r.get("error"))
                        .and_then(|e| e.get("message"))
                        .and_then(|v| v.as_str())
                })
                .or_else(|| {
                    event
                        .get("response")
                        .and_then(|r| r.get("incomplete_details"))
                        .and_then(|d| d.get("reason"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("unknown error")
                .to_string(),
        ),
        _ => None,
    }
}

fn block_key(kind: &str, index: usize) -> String {
    format!("{kind}:{index}")
}

fn block_model_item_id(item_prefix: &str, kind: &str, index: usize) -> String {
    format!("{item_prefix}-response-{index}-{kind}")
}

async fn send_model_block_event(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    event: ModelBlockStreamEvent,
) -> bool {
    tx.send(Ok(event)).await.is_ok()
}

async fn start_output_item(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = output_index(event);
    let item = event.get("item").unwrap_or(&Value::Null);
    let item_type = item_type(item);
    match item_type {
        "reasoning" => ensure_reasoning_block(tx, active_blocks, event, item_prefix).await,
        "message" => ensure_text_block(tx, active_blocks, event, item_prefix).await,
        "function_call" => ensure_tool_block(tx, active_blocks, event, item_prefix).await,
        _ => {
            let _ = index;
            true
        }
    }
}

async fn send_text_delta(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    if !ensure_text_block(tx, active_blocks, event, item_prefix).await {
        return false;
    }
    let index = output_index(event);
    let key = block_key("text", index);
    let Some(ActiveResponsesBlock::AssistantText { model_item_id }) = active_blocks.get(&key)
    else {
        return true;
    };
    let Some(delta) = event.get("delta").and_then(|v| v.as_str()) else {
        return true;
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockDelta {
            model_item_id: model_item_id.clone(),
            delta: ModelBlockDelta::AssistantText {
                delta: delta.to_string(),
            },
        },
    )
    .await
}

async fn send_reasoning_delta(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    if !ensure_reasoning_block(tx, active_blocks, event, item_prefix).await {
        return false;
    }
    let index = output_index(event);
    let key = block_key("reasoning", index);
    let Some(ActiveResponsesBlock::Reasoning { model_item_id }) = active_blocks.get(&key) else {
        return true;
    };
    let Some(delta) = event.get("delta").and_then(|v| v.as_str()) else {
        return true;
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockDelta {
            model_item_id: model_item_id.clone(),
            delta: ModelBlockDelta::ReasoningText {
                delta: delta.to_string(),
            },
        },
    )
    .await
}

async fn send_tool_delta(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    if !ensure_tool_block(tx, active_blocks, event, item_prefix).await {
        return false;
    }
    let index = output_index(event);
    let key = block_key("tool", index);
    let Some(ActiveResponsesBlock::ToolCall { model_item_id }) = active_blocks.get(&key) else {
        return true;
    };
    let Some(delta) = event.get("delta").and_then(|v| v.as_str()) else {
        return true;
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockDelta {
            model_item_id: model_item_id.clone(),
            delta: ModelBlockDelta::ToolArguments {
                delta: delta.to_string(),
            },
        },
    )
    .await
}

async fn ensure_reasoning_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = output_index(event);
    let key = block_key("reasoning", index);
    if active_blocks.contains_key(&key) {
        return true;
    }
    let item = event.get("item").unwrap_or(&Value::Null);
    let model_item_id = block_model_item_id(item_prefix, "reasoning", index);
    active_blocks.insert(
        key,
        ActiveResponsesBlock::Reasoning {
            model_item_id: model_item_id.clone(),
        },
    );
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id,
            provider_item_id: provider_item_id(item, index),
            kind: ModelBlockKind::Reasoning,
            assistant_text_phase_hint: None,
            assistant_text_phase_hint_source: None,
            tool_call: None,
        }),
    )
    .await
}

async fn ensure_text_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = output_index(event);
    let key = block_key("text", index);
    if active_blocks.contains_key(&key) {
        return true;
    }
    let item = event.get("item").unwrap_or(&Value::Null);
    let model_item_id = block_model_item_id(item_prefix, "text", index);
    active_blocks.insert(
        key,
        ActiveResponsesBlock::AssistantText {
            model_item_id: model_item_id.clone(),
        },
    );
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id,
            provider_item_id: provider_item_id(item, index),
            kind: ModelBlockKind::AssistantText,
            assistant_text_phase_hint: None,
            assistant_text_phase_hint_source: None,
            tool_call: None,
        }),
    )
    .await
}

async fn ensure_tool_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = output_index(event);
    let key = block_key("tool", index);
    if active_blocks.contains_key(&key) {
        return true;
    }
    let item = event.get("item").unwrap_or(&Value::Null);
    let model_item_id = block_model_item_id(item_prefix, "tool", index);
    active_blocks.insert(
        key,
        ActiveResponsesBlock::ToolCall {
            model_item_id: model_item_id.clone(),
        },
    );
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id,
            provider_item_id: provider_item_id(item, index),
            kind: ModelBlockKind::ToolCall,
            assistant_text_phase_hint: None,
            assistant_text_phase_hint_source: None,
            tool_call: Some(ModelToolCallStart {
                index,
                call_id: item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                tool_name: item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            }),
        }),
    )
    .await
}

async fn complete_output_item(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
) -> bool {
    let index = output_index(event);
    let item = event.get("item").unwrap_or(&Value::Null);
    let key = match item_type(item) {
        "reasoning" => block_key("reasoning", index),
        "message" => block_key("text", index),
        "function_call" => block_key("tool", index),
        _ => return true,
    };
    complete_block(tx, active_blocks, &key).await
}

async fn complete_open_blocks(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
) -> bool {
    let keys: Vec<String> = active_blocks.keys().cloned().collect();
    for key in keys {
        if !complete_block(tx, active_blocks, &key).await {
            return false;
        }
    }
    true
}

async fn complete_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    key: &str,
) -> bool {
    let Some(active_block) = active_blocks.remove(key) else {
        return true;
    };
    let model_item_id = match active_block {
        ActiveResponsesBlock::Reasoning { model_item_id }
        | ActiveResponsesBlock::AssistantText { model_item_id }
        | ActiveResponsesBlock::ToolCall { model_item_id } => model_item_id,
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted { model_item_id }),
    )
    .await
}

fn provider_item_id(item: &Value, index: usize) -> Option<String> {
    item.get("id")
        .or_else(|| item.get("call_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| Some(format!("output:{index}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_request_serializes_tools_reasoning_and_function_output() {
        let request = request(
            ChatRequest::builder()
                .model("gpt-5")
                .messages(vec![
                    ChatMessage::system("System"),
                    ChatMessage::user("Hello"),
                    ChatMessage::assistant_with_tool_calls(
                        "Using a tool",
                        vec![FunctionCall::new(
                            "call_1",
                            "read",
                            serde_json::json!({"path": "/tmp/a"}),
                        )],
                    ),
                    ChatMessage::tool("call_1", "read", "file body"),
                ])
                .max_tokens(100)
                .tools(vec![ToolDefinition::new(
                    "read",
                    "Read file",
                    Some(serde_json::json!({"type": "object"})),
                )])
                .tool_choice(ToolChoice::Named {
                    name: "read".to_string(),
                })
                .parallel_tool_calls(true)
                .reasoning_effort(ReasoningEffort::High)
                .build(),
            Some(true),
        );

        let json = serde_json::to_value(&request).expect("request should serialize");

        assert_eq!(json["model"], "gpt-5");
        assert_eq!(json["max_output_tokens"], 100);
        assert_eq!(json["stream"], true);
        assert_eq!(json["tools"][0]["type"], "function");
        assert_eq!(json["tool_choice"]["name"], "read");
        assert_eq!(json["reasoning"]["effort"], "high");
        assert!(json["input"]
            .as_array()
            .expect("input should be array")
            .iter()
            .any(|item| item["type"] == "function_call_output" && item["call_id"] == "call_1"));
    }

    #[test]
    fn responses_chat_response_reads_output_items() {
        let response: ResponsesResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_1",
            "model": "gpt-5",
            "status": "completed",
            "output": [
                {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "thinking"}]
                },
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "hello"}]
                },
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "read",
                    "arguments": "{\"path\":\"/tmp/a\"}"
                }
            ],
            "usage": {
                "input_tokens": 1,
                "output_tokens": 2,
                "total_tokens": 3,
                "input_tokens_details": {"cached_tokens": 1}
            }
        }))
        .expect("response should parse");

        let mapped = chat_response(response);

        assert_eq!(mapped.content, "hello");
        assert_eq!(mapped.reasoning, vec!["thinking".to_string()]);
        assert_eq!(mapped.usage.total_tokens, 3);
        assert_eq!(mapped.usage.cache_read_input_tokens, Some(1));
        let calls = mapped.tool_calls.expect("tool calls should exist");
        assert_eq!(calls[0].name, "read");
        assert_eq!(calls[0].arguments["path"], "/tmp/a");
        assert_eq!(mapped.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn responses_chat_response_does_not_duplicate_output_text() {
        let response: ResponsesResponse = serde_json::from_value(serde_json::json!({
            "id": "resp_1",
            "model": "gpt-5",
            "status": "completed",
            "output_text": "hello",
            "output": [
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "hello"}]
                }
            ]
        }))
        .expect("response should parse");

        let mapped = chat_response(response);

        assert_eq!(mapped.content, "hello");
    }
}
