use crate::{
    reasoning_budget_tokens, CacheControl, ChatChunk, ChatMessage, ChatRequest, ChatResponse,
    ChatStream, FunctionCall, ModelBlockCompleted, ModelBlockDelta, ModelBlockKind,
    ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent, ModelToolCallStart,
    ReasoningEffort, Role, SseLineDecoder, ToolChoice, ToolDefinition, Usage,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesMessage {
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessagesTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "input_schema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagesToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct MessagesRequest {
    pub model: String,
    pub messages: Vec<MessagesMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<Vec<MessagesTextBlock>>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<MessagesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<MessagesToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<MessagesThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessagesThinkingConfig {
    pub r#type: String,
    pub budget_tokens: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessagesResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default, rename = "type")]
    pub response_type: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    pub content: Vec<MessagesContent>,
    pub model: String,
    pub stop_reason: Option<String>,
    pub usage: MessagesUsage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessagesContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessagesUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessagesTextBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

pub fn text_block(text: String, cache_control: Option<CacheControl>) -> Value {
    serde_json::to_value(MessagesTextBlock {
        block_type: "text".to_string(),
        text,
        cache_control,
    })
    .expect("messages text block should serialize")
}

pub fn tool_choice(choice: Option<ToolChoice>) -> Option<MessagesToolChoice> {
    match choice {
        Some(ToolChoice::Auto) => Some(MessagesToolChoice::Auto),
        Some(ToolChoice::Required) => Some(MessagesToolChoice::Any),
        Some(ToolChoice::Named { name }) => Some(MessagesToolChoice::Tool { name }),
        Some(ToolChoice::None) | None => None,
    }
}

pub fn tools(tools: Option<Vec<ToolDefinition>>) -> Option<Vec<MessagesTool>> {
    tools.map(|defs| {
        defs.into_iter()
            .map(|tool| MessagesTool {
                name: tool.name,
                description: tool.description,
                input_schema: tool
                    .parameters
                    .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
            })
            .collect()
    })
}

pub fn messages(
    messages: Vec<ChatMessage>,
) -> (Option<Vec<MessagesTextBlock>>, Vec<MessagesMessage>) {
    let mut system_blocks: Option<Vec<MessagesTextBlock>> = None;
    let mut protocol_messages = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                system_blocks
                    .get_or_insert_with(Vec::new)
                    .push(MessagesTextBlock {
                        block_type: "text".to_string(),
                        text: msg.content,
                        cache_control: msg.cache_control,
                    });
            }
            Role::User => {
                protocol_messages.push(MessagesMessage {
                    role: "user".to_string(),
                    content: Value::Array(vec![text_block(msg.content, msg.cache_control)]),
                });
            }
            Role::Assistant => {
                if let Some(tool_calls) = msg.tool_calls {
                    let mut blocks = Vec::new();
                    if !msg.content.is_empty() {
                        blocks.push(text_block(msg.content, msg.cache_control));
                    }
                    for call in tool_calls {
                        blocks.push(serde_json::json!({
                            "type": "tool_use",
                            "id": call.id,
                            "name": call.name,
                            "input": call.arguments,
                        }));
                    }
                    protocol_messages.push(MessagesMessage {
                        role: "assistant".to_string(),
                        content: Value::Array(blocks),
                    });
                } else {
                    protocol_messages.push(MessagesMessage {
                        role: "assistant".to_string(),
                        content: Value::Array(vec![text_block(msg.content, msg.cache_control)]),
                    });
                }
            }
            Role::Tool => {
                let tool_use_id = msg.tool_call_id.unwrap_or_default();
                protocol_messages.push(MessagesMessage {
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

    (system_blocks, protocol_messages)
}

pub fn response_content(
    content: &[MessagesContent],
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

pub fn request(request: ChatRequest, stream: Option<bool>) -> MessagesRequest {
    let (system, messages) = messages(request.messages);
    MessagesRequest {
        model: request.model,
        messages,
        system,
        max_tokens: request.max_tokens.unwrap_or(1024),
        temperature: request.temperature,
        top_p: request.top_p,
        stop_sequences: request.stop,
        tools: tools(request.tools),
        tool_choice: tool_choice(request.tool_choice),
        thinking: request.reasoning_effort.map(thinking),
        stream,
    }
}

pub fn thinking(effort: ReasoningEffort) -> MessagesThinkingConfig {
    MessagesThinkingConfig {
        r#type: "enabled".to_string(),
        budget_tokens: reasoning_budget_tokens(effort),
    }
}

pub fn chat_response(response: MessagesResponse) -> ChatResponse {
    let (content, reasoning, tool_calls) = response_content(&response.content);
    ChatResponse {
        content,
        reasoning,
        model: response.model,
        usage: Usage {
            prompt_tokens: response.usage.input_tokens,
            completion_tokens: response.usage.output_tokens,
            total_tokens: response.usage.input_tokens + response.usage.output_tokens,
            cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
            cache_read_input_tokens: response.usage.cache_read_input_tokens,
        },
        finish_reason: response.stop_reason,
        tool_calls,
    }
}

pub fn stream(response: reqwest::Response, provider_name: impl Into<String>) -> ChatStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);
    let provider_name = provider_name.into();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut stop_reason: Option<String> = None;
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
                        };
                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err(TalosError::InternalError(format!(
                                        "{provider_name} stream invalid SSE JSON: {error}"
                                    ))))
                                    .await;
                                return;
                            }
                        };

                        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match event_type {
                            "error" => {
                                let msg = stream_error_message(&event);
                                let _ = tx
                                    .send(Err(TalosError::InternalError(format!(
                                        "{provider_name} stream error: {msg}"
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
    provider_name: impl Into<String>,
    item_prefix: impl Into<String>,
) -> ModelBlockStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(100);
    let provider_name = provider_name.into();
    let item_prefix = item_prefix.into();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut active_blocks: HashMap<usize, ActiveMessagesBlock> = HashMap::new();
        let mut stop_reason: Option<String> = None;
        let mut output_tokens: Option<u32> = None;
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
                        };
                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err(TalosError::InternalError(format!(
                                        "{provider_name} model block stream invalid SSE JSON: {error}"
                                    ))))
                                    .await;
                                return;
                            }
                        };

                        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match event_type {
                            "error" => {
                                let msg = stream_error_message(&event);
                                let _ = tx
                                    .send(Err(TalosError::InternalError(format!(
                                        "{provider_name} stream error: {msg}"
                                    ))))
                                    .await;
                                return;
                            }
                            "content_block_start" => {
                                if !start_content_block(
                                    &tx,
                                    &mut active_blocks,
                                    &event,
                                    &item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "content_block_delta" => {
                                if !send_content_block_delta(&tx, &active_blocks, &event).await {
                                    return;
                                }
                            }
                            "content_block_stop" => {
                                if !stop_content_block(&tx, &mut active_blocks, &event).await {
                                    return;
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
                                if !active_blocks.is_empty() {
                                    let _ = tx
                                        .send(Err(TalosError::InternalError(format!(
                                            "{provider_name} stream stopped with active content blocks"
                                        ))))
                                        .await;
                                    return;
                                }
                                let usage = output_tokens.map(|out| Usage {
                                    prompt_tokens: 0,
                                    completion_tokens: out,
                                    total_tokens: out,
                                    cache_creation_input_tokens: None,
                                    cache_read_input_tokens: None,
                                });
                                let _ = tx
                                    .send(Ok(ModelBlockStreamEvent::Done {
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

#[derive(Debug, Clone)]
enum ActiveMessagesBlock {
    Reasoning { model_item_id: String },
    AssistantText { model_item_id: String },
    ToolCall { model_item_id: String },
}

fn block_model_item_id(item_prefix: &str, index: usize, kind: &str) -> String {
    format!("{item_prefix}-block-{index}-{kind}")
}

fn stream_error_message(event: &Value) -> &str {
    event
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown error")
}

async fn send_model_block_event(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    event: ModelBlockStreamEvent,
) -> bool {
    tx.send(Ok(event)).await.is_ok()
}

async fn start_content_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<usize, ActiveMessagesBlock>,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let block = event.get("content_block");
    let block_type = block
        .and_then(|b| b.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let provider_item_id = block
        .and_then(|b| b.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| Some(format!("content_block:{index}")));

    let (active, started) = match block_type {
        "thinking" => {
            let model_item_id = block_model_item_id(item_prefix, index, "thinking");
            (
                ActiveMessagesBlock::Reasoning {
                    model_item_id: model_item_id.clone(),
                },
                ModelBlockStarted {
                    model_item_id,
                    provider_item_id,
                    kind: ModelBlockKind::Reasoning,
                    assistant_text_phase_hint: None,
                    assistant_text_phase_hint_source: None,
                    tool_call: None,
                },
            )
        }
        "text" => {
            let model_item_id = block_model_item_id(item_prefix, index, "text");
            (
                ActiveMessagesBlock::AssistantText {
                    model_item_id: model_item_id.clone(),
                },
                ModelBlockStarted {
                    model_item_id,
                    provider_item_id,
                    kind: ModelBlockKind::AssistantText,
                    assistant_text_phase_hint: None,
                    assistant_text_phase_hint_source: None,
                    tool_call: None,
                },
            )
        }
        "tool_use" => {
            let call_id = block
                .and_then(|b| b.get("id"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let tool_name = block
                .and_then(|b| b.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let model_item_id = block_model_item_id(item_prefix, index, "tool");
            (
                ActiveMessagesBlock::ToolCall {
                    model_item_id: model_item_id.clone(),
                },
                ModelBlockStarted {
                    model_item_id,
                    provider_item_id: call_id.clone(),
                    kind: ModelBlockKind::ToolCall,
                    assistant_text_phase_hint: None,
                    assistant_text_phase_hint_source: None,
                    tool_call: Some(ModelToolCallStart {
                        index,
                        call_id,
                        tool_name,
                    }),
                },
            )
        }
        _ => return true,
    };
    active_blocks.insert(index, active);
    send_model_block_event(tx, ModelBlockStreamEvent::BlockStarted(started)).await
}

async fn send_content_block_delta(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &HashMap<usize, ActiveMessagesBlock>,
    event: &Value,
) -> bool {
    let index = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let delta = event.get("delta");
    let delta_type = delta
        .and_then(|d| d.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let Some(active_block) = active_blocks.get(&index) else {
        return true;
    };
    let mapped_delta = match (delta_type, active_block) {
        ("thinking_delta", ActiveMessagesBlock::Reasoning { model_item_id }) => delta
            .and_then(|d| d.get("thinking"))
            .and_then(|v| v.as_str())
            .map(|text| {
                (
                    model_item_id.clone(),
                    ModelBlockDelta::ReasoningText {
                        delta: text.to_string(),
                    },
                )
            }),
        ("text_delta", ActiveMessagesBlock::AssistantText { model_item_id }) => delta
            .and_then(|d| d.get("text"))
            .and_then(|v| v.as_str())
            .map(|text| {
                (
                    model_item_id.clone(),
                    ModelBlockDelta::AssistantText {
                        delta: text.to_string(),
                    },
                )
            }),
        ("input_json_delta", ActiveMessagesBlock::ToolCall { model_item_id }) => delta
            .and_then(|d| d.get("partial_json"))
            .and_then(|v| v.as_str())
            .map(|partial_json| {
                (
                    model_item_id.clone(),
                    ModelBlockDelta::ToolArguments {
                        delta: partial_json.to_string(),
                    },
                )
            }),
        _ => None,
    };

    if let Some((model_item_id, delta)) = mapped_delta {
        send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockDelta {
                model_item_id,
                delta,
            },
        )
        .await
    } else {
        true
    }
}

async fn stop_content_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<usize, ActiveMessagesBlock>,
    event: &Value,
) -> bool {
    let index = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let Some(active_block) = active_blocks.remove(&index) else {
        return true;
    };
    let model_item_id = match active_block {
        ActiveMessagesBlock::Reasoning { model_item_id }
        | ActiveMessagesBlock::AssistantText { model_item_id }
        | ActiveMessagesBlock::ToolCall { model_item_id } => model_item_id,
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted { model_item_id }),
    )
    .await
}
