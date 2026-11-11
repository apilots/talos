use crate::{
    ChatChunk, ChatMessage, ChatRequest, ChatStream, ModelBlockCompleted, ModelBlockDelta,
    ModelBlockKind, ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent, ModelOutputPhase,
    ModelToolCallStart, ReasoningContinuation, ReasoningOutput, Role, SseLineDecoder, ToolChoice,
    ToolDefinition,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;

mod attachments;
mod decode;
mod reasoning;
mod terminal;
mod types;

use decode::reasoning_from_item;
pub use decode::{chat_response, official_chat_response, response_content, usage};
use terminal::{stream_terminal, unterminated_stream};
pub use types::*;

#[derive(Debug, Clone)]
enum ActiveResponsesBlock {
    Reasoning {
        model_item_id: String,
    },
    AssistantText {
        model_item_id: String,
    },
    ToolCall {
        model_item_id: String,
        arguments: String,
    },
}

pub fn request(
    request: ChatRequest,
    stream: Option<bool>,
    options: ResponsesRequestOptions,
) -> TalosResult<ResponsesRequest> {
    request_with_contract(request, stream, options, ResponsesContract::Compatible)
}

/// Encodes a request using the selected endpoint's reasoning replay contract.
pub fn request_with_contract(
    request: ChatRequest,
    stream: Option<bool>,
    options: ResponsesRequestOptions,
    contract: ResponsesContract,
) -> TalosResult<ResponsesRequest> {
    Ok(ResponsesRequest {
        model: request.model,
        input: input_with_contract(request.messages, contract)?,
        temperature: request.temperature,
        max_output_tokens: request.max_tokens,
        top_p: request.top_p,
        tools: tools(request.tools),
        tool_choice: tool_choice(request.tool_choice),
        parallel_tool_calls: request.parallel_tool_calls,
        reasoning: options.reasoning,
        include: options.include,
        store: None,
        stream,
    })
}

pub fn input(messages: Vec<ChatMessage>) -> TalosResult<Vec<Value>> {
    input_with_contract(messages, ResponsesContract::Compatible)
}

fn input_with_contract(
    messages: Vec<ChatMessage>,
    contract: ResponsesContract,
) -> TalosResult<Vec<Value>> {
    let mut items = Vec::new();

    for message in messages {
        message.validate_attachments()?;
        match message.role {
            Role::System => {
                items.push(serde_json::json!({
                    "role": "system",
                    "content": [{
                        "type": "input_text",
                        "text": message.content,
                    }],
                }));
            }
            Role::User => {
                items.push(serde_json::json!({
                    "role": "user",
                    "content": attachments::user_content(
                        message.content,
                        message.attachments,
                    ),
                }));
            }
            Role::Assistant => {
                items.extend(reasoning::items(message.reasoning_continuation, contract)?);
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
                    "output": attachments::tool_content(message.content, message.attachments),
                }));
            }
        }
    }

    Ok(items)
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
                strict: None,
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

pub fn stream(response: reqwest::Response, provider_name: impl Into<String>) -> ChatStream {
    stream_with_contract(response, provider_name, ResponsesContract::Compatible)
}

/// Decodes text events using the selected endpoint's termination contract.
pub fn stream_with_contract(
    response: reqwest::Response,
    provider_name: impl Into<String>,
    contract: ResponsesContract,
) -> ChatStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);
    let provider_name = provider_name.into();
    let official = contract == ResponsesContract::Official;

    tokio::spawn(async move {
        let request_context = crate::error::response_context(&response);
        let mut stream = response.bytes_stream();
        let mut decoder = SseLineDecoder::new();

        while let Some(chunk_result) =
            tokio::select! { _ = tx.closed() => return, next = stream.next() => next }
        {
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
                            let result = if official {
                                Err(unterminated_stream(&request_context))
                            } else {
                                Ok(ChatChunk {
                                    delta: String::new(),
                                    finish_reason: Some("stop".to_string()),
                                })
                            };
                            let _ = tx.send(result).await;
                            return;
                        }

                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err({ let diagnostic = format!(
                                        "{provider_name} responses stream invalid SSE JSON: {error}"
                                    ); TalosError::internal_with_source("Model responses stream invalid SSE JSON; technical details were withheld.", error).context(diagnostic) }))
                                    .await;
                                return;
                            }
                        };

                        if let Some(terminal) = stream_terminal(&event, false, contract) {
                            let result = terminal.map(|finish_reason| ChatChunk {
                                delta: String::new(),
                                finish_reason: Some(finish_reason),
                            });
                            let _ = tx
                                .send(result.map_err(|error| error.context(&request_context)))
                                .await;
                            return;
                        }

                        let kind = event_type(&event);
                        if kind == "response.output_text.delta"
                            || (official && kind == "response.refusal.delta")
                        {
                            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                                let _ = tx
                                    .send(Ok(ChatChunk {
                                        delta: delta.to_string(),
                                        finish_reason: None,
                                    }))
                                    .await;
                            }
                        }
                    }
                }
                Err(error) => {
                    let error = crate::error::transport_error("Model response stream", error)
                        .context(&request_context);
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            }
        }
        if official {
            let _ = tx.send(Err(unterminated_stream(&request_context))).await;
        }
    });

    ChatStream::new(rx)
}

pub fn model_blocks(
    response: reqwest::Response,
    options: ResponsesModelBlocks,
) -> ModelBlockStream {
    model_blocks_with_contract(response, options, ResponsesContract::Compatible)
}

/// Decodes model blocks using the selected endpoint's termination and tool contract.
pub fn model_blocks_with_contract(
    response: reqwest::Response,
    options: ResponsesModelBlocks,
    contract: ResponsesContract,
) -> ModelBlockStream {
    let official = contract == ResponsesContract::Official;
    let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(100);

    tokio::spawn(async move {
        let request_context = crate::error::response_context(&response);
        let mut stream = response.bytes_stream();
        let mut decoder = SseLineDecoder::new();
        let mut active_blocks: HashMap<String, ActiveResponsesBlock> = HashMap::new();
        let mut next_tool_index = 0;
        let mut reasoning_output = ReasoningOutput::default();

        while let Some(chunk_result) =
            tokio::select! { _ = tx.closed() => return, next = stream.next() => next }
        {
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
                            if official {
                                let _ = tx.send(Err(unterminated_stream(&request_context))).await;
                                return;
                            }
                            if !complete_open_blocks(&tx, &mut active_blocks).await {
                                return;
                            }
                            if !send_reasoning_output(&tx, &mut reasoning_output).await {
                                return;
                            }
                            let _ = tx
                                .send(Ok(ModelBlockStreamEvent::Done {
                                    finish_reason: Some("stop".to_string()),
                                    usage: None,
                                }))
                                .await;
                            return;
                        }

                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err({ let diagnostic = format!(
                                        "{} responses model block stream invalid SSE JSON: {error}",
                                        options.provider_name
                                    ); TalosError::internal_with_source("Model responses model block stream invalid SSE JSON; technical details were withheld.", error).context(diagnostic) }))
                                    .await;
                                return;
                            }
                        };

                        if let Some(terminal) =
                            stream_terminal(&event, next_tool_index != 0, contract)
                        {
                            let finish_reason = match terminal {
                                Ok(reason) => reason,
                                Err(error) => {
                                    let error = if official {
                                        error.context(&request_context)
                                    } else {
                                        let diagnostic = format!(
                                            "{} responses stream error: {error}",
                                            options.provider_name
                                        );
                                        TalosError::internal_with_source(
                                            "Model responses stream error; technical details were withheld.",
                                            error,
                                        )
                                        .context(diagnostic)
                                    };
                                    let _ = tx.send(Err(error)).await;
                                    return;
                                }
                            };
                            let final_usage = event
                                .get("response")
                                .and_then(|r| r.get("usage"))
                                .and_then(|value| {
                                    serde_json::from_value::<ResponsesUsage>(value.clone()).ok()
                                })
                                .map(|value| usage(Some(value)));
                            if complete_open_blocks(&tx, &mut active_blocks).await
                                && send_reasoning_output(&tx, &mut reasoning_output).await
                            {
                                let _ = tx
                                    .send(Ok(ModelBlockStreamEvent::Done {
                                        finish_reason: Some(finish_reason),
                                        usage: final_usage,
                                    }))
                                    .await;
                            }
                            return;
                        }

                        match event_type(&event) {
                            "response.output_item.added" => {
                                if !start_output_item(
                                    &tx,
                                    &mut active_blocks,
                                    &mut next_tool_index,
                                    &event,
                                    &options.item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            kind if kind == "response.output_text.delta"
                                || (official && kind == "response.refusal.delta") =>
                            {
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
                                    &mut next_tool_index,
                                    &event,
                                    &options.item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "response.function_call_arguments.done" if official => {
                                let key = block_key("tool", output_index(&event));
                                if !reconcile_tool_arguments(
                                    &tx,
                                    active_blocks.get_mut(&key),
                                    event["arguments"].as_str(),
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "response.output_item.done" => {
                                append_reasoning_output(&mut reasoning_output, &event);
                                if !complete_output_item(&tx, &mut active_blocks, &event, contract)
                                    .await
                                {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Err(error) => {
                    let error = crate::error::transport_error("Model response stream", error)
                        .context(&request_context);
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            }
        }
        if official {
            let _ = tx.send(Err(unterminated_stream(&request_context))).await;
        }
    });

    ModelBlockStream::new(rx)
}

fn item_type(item: &Value) -> &str {
    item.get("type").and_then(Value::as_str).unwrap_or("")
}

fn event_type(event: &Value) -> &str {
    event.get("type").and_then(Value::as_str).unwrap_or("")
}

fn output_index(event: &Value) -> usize {
    event
        .get("output_index")
        .or_else(|| event.get("index"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
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
    next_tool_index: &mut usize,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let item = event.get("item").unwrap_or(&Value::Null);
    match item_type(item) {
        "reasoning" => ensure_reasoning_block(tx, active_blocks, event, item_prefix).await,
        "message" => ensure_text_block(tx, active_blocks, event, item_prefix).await,
        "function_call" => {
            ensure_tool_block(tx, active_blocks, next_tool_index, event, item_prefix).await
        }
        _ => true,
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
    let Some(delta) = event.get("delta").and_then(Value::as_str) else {
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
    let Some(delta) = event.get("delta").and_then(Value::as_str) else {
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
    next_tool_index: &mut usize,
    event: &Value,
    item_prefix: &str,
) -> bool {
    if !ensure_tool_block(tx, active_blocks, next_tool_index, event, item_prefix).await {
        return false;
    }
    let index = output_index(event);
    let key = block_key("tool", index);
    let Some(ActiveResponsesBlock::ToolCall {
        model_item_id,
        arguments,
    }) = active_blocks.get_mut(&key)
    else {
        return true;
    };
    let Some(delta) = event.get("delta").and_then(Value::as_str) else {
        return true;
    };
    arguments.push_str(delta);
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
            output_phase: ModelOutputPhase::Unknown,
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
    let output_phase = ModelOutputPhase::deserialize(item.get("phase").unwrap_or(&Value::Null))
        .unwrap_or_default();
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
            output_phase,
            tool_call: None,
        }),
    )
    .await
}

async fn ensure_tool_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    next_tool_index: &mut usize,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = output_index(event);
    let key = block_key("tool", index);
    if active_blocks.contains_key(&key) {
        return true;
    }
    // Wire output indices include reasoning and text items; the kernel expects
    // a separate contiguous ordinal for tools only. Keep wire identity below.
    let tool_index = *next_tool_index;
    *next_tool_index += 1;
    let item = event.get("item").unwrap_or(&Value::Null);
    let model_item_id = block_model_item_id(item_prefix, "tool", index);
    active_blocks.insert(
        key,
        ActiveResponsesBlock::ToolCall {
            model_item_id: model_item_id.clone(),
            arguments: String::new(),
        },
    );
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id,
            provider_item_id: provider_item_id(item, index),
            kind: ModelBlockKind::ToolCall,
            output_phase: ModelOutputPhase::Unknown,
            tool_call: Some(ModelToolCallStart {
                index: tool_index,
                call_id: item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                tool_name: item.get("name").and_then(Value::as_str).map(str::to_string),
            }),
        }),
    )
    .await
}

async fn complete_output_item(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_blocks: &mut HashMap<String, ActiveResponsesBlock>,
    event: &Value,
    contract: ResponsesContract,
) -> bool {
    let index = output_index(event);
    let item = event.get("item").unwrap_or(&Value::Null);
    let key = match item_type(item) {
        "reasoning" => block_key("reasoning", index),
        "message" => block_key("text", index),
        "function_call" if contract == ResponsesContract::Official => {
            if item["status"] == "incomplete" {
                let _ = tx
                    .send(Err(TalosError::internal(
                        "The model response contained an incomplete tool call.",
                    )))
                    .await;
                return false;
            }
            let key = block_key("tool", index);
            if !reconcile_tool_arguments(
                tx,
                active_blocks.get_mut(&key),
                item["arguments"].as_str(),
            )
            .await
            {
                return false;
            }
            key
        }
        "function_call" => block_key("tool", index),
        _ => return true,
    };
    complete_block(tx, active_blocks, &key).await
}

async fn reconcile_tool_arguments(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    active_block: Option<&mut ActiveResponsesBlock>,
    final_arguments: Option<&str>,
) -> bool {
    let (
        Some(ActiveResponsesBlock::ToolCall {
            model_item_id,
            arguments,
        }),
        Some(final_arguments),
    ) = (active_block, final_arguments)
    else {
        return true;
    };
    let Some(suffix) = final_arguments.strip_prefix(arguments.as_str()) else {
        let _ = tx
            .send(Err(TalosError::internal(
                "The completed model tool arguments differ from the streamed arguments.",
            )))
            .await;
        return false;
    };
    if suffix.is_empty() {
        return true;
    }
    let delta = suffix.to_string();
    arguments.push_str(&delta);
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockDelta {
            model_item_id: model_item_id.clone(),
            delta: ModelBlockDelta::ToolArguments { delta },
        },
    )
    .await
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
        | ActiveResponsesBlock::ToolCall { model_item_id, .. } => model_item_id,
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
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| Some(format!("output:{index}")))
}

fn append_reasoning_output(output: &mut ReasoningOutput, event: &Value) {
    let item = event.get("item").unwrap_or(&Value::Null);
    if item_type(item) != "reasoning" {
        return;
    }
    let (display_text, blocks) = reasoning_from_item(item);
    output.display_text.extend(display_text);
    if blocks.is_empty() {
        return;
    }
    match output
        .continuation
        .get_or_insert_with(|| ReasoningContinuation::Blocks(Vec::new()))
    {
        ReasoningContinuation::Blocks(existing) => existing.extend(blocks),
        ReasoningContinuation::Text(_) => {
            unreachable!("Responses protocol only builds structured reasoning continuation")
        }
    }
}

async fn send_reasoning_output(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    output: &mut ReasoningOutput,
) -> bool {
    if output.continuation.is_none() {
        return true;
    }
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::ReasoningCompleted {
            output: std::mem::take(output),
        },
    )
    .await
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
