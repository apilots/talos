use crate::{
    ChatChunk, ChatMessage, ChatRequest, ChatStream, FunctionCall, ModelBlockCompleted,
    ModelBlockDelta, ModelBlockKind, ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent,
    ModelOutputPhase, ModelToolCallStart, ReasoningBlock, ReasoningContinuation,
    ReasoningEncryptedBlock, ReasoningOutput, ReasoningTextBlock, Role, SseLineDecoder, ToolChoice,
    ToolDefinition,
};
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;
mod attachments;
mod types;
mod usage;

pub use attachments::text_block;
pub use types::*;
pub use usage::{chat_response, MessagesUsage};
use usage::{stream_input_usage, usage_from_messages};

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
) -> TalosResult<(Option<Vec<MessagesTextBlock>>, Vec<MessagesMessage>)> {
    let mut system_blocks: Option<Vec<MessagesTextBlock>> = None;
    let mut protocol_messages = Vec::new();

    for msg in messages {
        msg.validate_attachments()?;
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
                    content: Value::Array(attachments::user_content(
                        msg.content,
                        msg.cache_control,
                        msg.attachments,
                    )),
                });
            }
            Role::Assistant => {
                let mut blocks = reasoning_blocks(msg.reasoning_continuation)?;
                if !msg.content.is_empty() {
                    blocks.push(text_block(msg.content, msg.cache_control));
                }
                if let Some(tool_calls) = msg.tool_calls {
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
                        content: Value::Array(blocks),
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
                        "content": attachments::tool_content(msg.content, msg.attachments),
                        "is_error": false,
                    })]),
                });
            }
        }
    }

    Ok((system_blocks, protocol_messages))
}

fn reasoning_blocks(continuation: Option<ReasoningContinuation>) -> TalosResult<Vec<Value>> {
    let Some(continuation) = continuation else {
        return Ok(Vec::new());
    };

    Ok(match continuation {
        ReasoningContinuation::Text(text) => vec![serde_json::json!({
            "type": "thinking",
            "thinking": text,
        })],
        ReasoningContinuation::Blocks(blocks) => blocks
            .into_iter()
            .map(|block| match block {
                ReasoningBlock::Text(block) => Ok(serde_json::json!({
                    "type": "thinking",
                    "thinking": block.text,
                    "signature": block.signature,
                })),
                ReasoningBlock::Encrypted(block) => Ok(serde_json::json!({
                    "type": "redacted_thinking",
                    "data": block.data,
                })),
                ReasoningBlock::Summary(_) => Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Messages protocol cannot replay a reasoning summary block".to_string(),
                )),
            })
            .collect::<TalosResult<Vec<_>>>()?,
    })
}

pub fn response_content(
    content: &[MessagesContent],
) -> (String, ReasoningOutput, Option<Vec<FunctionCall>>) {
    let mut text = Vec::new();
    let mut display_text = Vec::new();
    let mut continuation = Vec::new();
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
                    display_text.push(fragment.clone());
                    continuation.push(ReasoningBlock::Text(ReasoningTextBlock {
                        provider_item_id: block.id.clone(),
                        provider_index: None,
                        text: fragment.clone(),
                        signature: block.signature.clone(),
                        format: Some("anthropic-claude-v1".to_string()),
                    }));
                }
            }
            "redacted_thinking" => {
                if let Some(data) = block.data.as_ref() {
                    continuation.push(ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
                        provider_item_id: block.id.clone(),
                        provider_index: None,
                        data: data.clone(),
                        format: Some("anthropic-claude-v1".to_string()),
                    }));
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
    let continuation =
        (!continuation.is_empty()).then_some(ReasoningContinuation::Blocks(continuation));
    (
        text.join(""),
        ReasoningOutput {
            display_text,
            continuation,
        },
        tool_calls,
    )
}

pub fn request(
    request: ChatRequest,
    stream: Option<bool>,
    options: MessagesRequestOptions,
) -> TalosResult<MessagesRequest> {
    let (system, messages) = messages(request.messages)?;
    Ok(MessagesRequest {
        model: request.model,
        messages,
        system,
        max_tokens: request.max_tokens.unwrap_or(1024),
        temperature: request.temperature,
        top_p: request.top_p,
        stop_sequences: request.stop,
        tools: tools(request.tools),
        tool_choice: tool_choice(request.tool_choice),
        thinking: options.thinking,
        output_config: options.output_config,
        stream,
    })
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;

pub fn stream(response: reqwest::Response, provider_name: impl Into<String>) -> ChatStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);
    let provider_name = provider_name.into();

    tokio::spawn(async move {
        let request_context = crate::error::response_context(&response);
        let mut stream = response.bytes_stream();
        let mut stop_reason: Option<String> = None;
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
                        };
                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err({ let diagnostic = format!(
                                        "{provider_name} stream invalid SSE JSON: {error}"
                                    ); TalosError::internal_with_source("Model response stream invalid SSE JSON; technical details were withheld.", error).context(diagnostic) }))
                                    .await;
                                return;
                            }
                        };

                        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
                        match event_type {
                            "error" => {
                                let error = crate::error::stream_response_error(
                                    event
                                        .get("error")
                                        .and_then(|error| error.get("type"))
                                        .and_then(Value::as_str),
                                );
                                let _ = tx.send(Err(error.context(provider_name))).await;
                                return;
                            }
                            "content_block_delta" => {
                                if let Some(delta) = event
                                    .get("delta")
                                    .and_then(|d| d.get("text"))
                                    .and_then(Value::as_str)
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
                                    .and_then(Value::as_str)
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
                    let error = crate::error::transport_error("Model response stream", error)
                        .context(&request_context);
                    let _ = tx.send(Err(error)).await;
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
        let request_context = crate::error::response_context(&response);
        let mut stream = response.bytes_stream();
        let mut active_blocks: HashMap<usize, ActiveMessagesBlock> = HashMap::new();
        let mut next_tool_index = 0;
        let mut reasoning_blocks: Vec<(usize, ReasoningBlock)> = Vec::new();
        let mut stop_reason: Option<String> = None;
        let mut input_usage: Option<MessagesUsage> = None;
        let mut output_tokens: Option<u32> = None;
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
                        };
                        let event = match serde_json::from_str::<Value>(data) {
                            Ok(event) => event,
                            Err(error) => {
                                let _ = tx
                                    .send(Err({ let diagnostic = format!(
                                        "{provider_name} model block stream invalid SSE JSON: {error}"
                                    ); TalosError::internal_with_source("Model response model block stream invalid SSE JSON; technical details were withheld.", error).context(diagnostic) }))
                                    .await;
                                return;
                            }
                        };

                        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
                        match event_type {
                            "error" => {
                                let error = crate::error::stream_response_error(
                                    event
                                        .get("error")
                                        .and_then(|error| error.get("type"))
                                        .and_then(Value::as_str),
                                );
                                let _ = tx.send(Err(error.context(provider_name))).await;
                                return;
                            }
                            "message_start" => match stream_input_usage(&event) {
                                Ok(usage) => input_usage = usage,
                                Err(error) => {
                                    let _ = tx.send(Err(error)).await;
                                    return;
                                }
                            },
                            "content_block_start" => {
                                if !start_content_block(
                                    &tx,
                                    &mut active_blocks,
                                    &mut next_tool_index,
                                    &event,
                                    &item_prefix,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "content_block_delta" => {
                                if !send_content_block_delta(&tx, &mut active_blocks, &event).await
                                {
                                    return;
                                }
                            }
                            "content_block_stop" => {
                                if !stop_content_block(
                                    &tx,
                                    &mut active_blocks,
                                    &mut reasoning_blocks,
                                    &event,
                                )
                                .await
                                {
                                    return;
                                }
                            }
                            "message_delta" => {
                                if let Some(reason) = event
                                    .get("delta")
                                    .and_then(|d| d.get("stop_reason"))
                                    .and_then(Value::as_str)
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
                                        .send(Err(TalosError::internal("Model response stream stopped with active content blocks.").context(format!(
                                            "{provider_name} stream stopped with active content blocks"
                                        ))))
                                        .await;
                                    return;
                                }
                                let usage = match (input_usage.take(), output_tokens) {
                                    (Some(input), Some(output)) => {
                                        match usage_from_messages(&input, output) {
                                            Ok(usage) => Some(usage),
                                            Err(error) => {
                                                let _ = tx.send(Err(error)).await;
                                                return;
                                            }
                                        }
                                    }
                                    _ => None,
                                };
                                if !reasoning_blocks.is_empty() {
                                    reasoning_blocks.sort_by_key(|(index, _)| *index);
                                    let blocks: Vec<_> = reasoning_blocks
                                        .drain(..)
                                        .map(|(_, block)| block)
                                        .collect();
                                    let display_text = blocks
                                        .iter()
                                        .filter_map(|block| match block {
                                            ReasoningBlock::Text(block) => Some(block.text.clone()),
                                            ReasoningBlock::Summary(block) => {
                                                Some(block.text.clone())
                                            }
                                            ReasoningBlock::Encrypted(_) => None,
                                        })
                                        .collect();
                                    if !send_model_block_event(
                                        &tx,
                                        ModelBlockStreamEvent::ReasoningCompleted {
                                            output: ReasoningOutput {
                                                display_text,
                                                continuation: Some(ReasoningContinuation::Blocks(
                                                    blocks,
                                                )),
                                            },
                                        },
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                }
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
                    let error = crate::error::transport_error("Model response stream", error)
                        .context(&request_context);
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            }
        }
    });

    ModelBlockStream::new(rx)
}

#[derive(Debug, Clone)]
enum ActiveMessagesBlock {
    Reasoning {
        model_item_id: String,
        provider_item_id: Option<String>,
        text: String,
        signature: String,
    },
    EncryptedReasoning {
        provider_item_id: Option<String>,
        data: String,
    },
    AssistantText {
        model_item_id: String,
    },
    ToolCall {
        model_item_id: String,
    },
}

fn block_model_item_id(item_prefix: &str, index: usize, kind: &str) -> String {
    format!("{item_prefix}-block-{index}-{kind}")
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
    next_tool_index: &mut usize,
    event: &Value,
    item_prefix: &str,
) -> bool {
    let index = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let block = event.get("content_block");
    let block_type = block
        .and_then(|b| b.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let provider_item_id = block
        .and_then(|b| b.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| Some(format!("content_block:{index}")));

    let (active, started) = match block_type {
        "thinking" => {
            let model_item_id = block_model_item_id(item_prefix, index, "thinking");
            (
                ActiveMessagesBlock::Reasoning {
                    model_item_id: model_item_id.clone(),
                    provider_item_id: provider_item_id.clone(),
                    text: block
                        .and_then(|b| b.get("thinking"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    signature: block
                        .and_then(|b| b.get("signature"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                },
                ModelBlockStarted {
                    model_item_id,
                    provider_item_id,
                    kind: ModelBlockKind::Reasoning,
                    output_phase: ModelOutputPhase::Unknown,
                    tool_call: None,
                },
            )
        }
        "redacted_thinking" => {
            active_blocks.insert(
                index,
                ActiveMessagesBlock::EncryptedReasoning {
                    provider_item_id,
                    data: block
                        .and_then(|b| b.get("data"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                },
            );
            return true;
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
                    output_phase: ModelOutputPhase::Unknown,
                    tool_call: None,
                },
            )
        }
        "tool_use" => {
            // Content indices include thinking/text blocks; tool ordinals do not.
            let tool_index = *next_tool_index;
            *next_tool_index += 1;
            let call_id = block
                .and_then(|b| b.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let tool_name = block
                .and_then(|b| b.get("name"))
                .and_then(Value::as_str)
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
                    output_phase: ModelOutputPhase::Unknown,
                    tool_call: Some(ModelToolCallStart {
                        index: tool_index,
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
    active_blocks: &mut HashMap<usize, ActiveMessagesBlock>,
    event: &Value,
) -> bool {
    let index = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let delta = event.get("delta");
    let delta_type = delta
        .and_then(|d| d.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let Some(active_block) = active_blocks.get_mut(&index) else {
        return true;
    };
    let mapped_delta = match (delta_type, active_block) {
        (
            "thinking_delta",
            ActiveMessagesBlock::Reasoning {
                model_item_id,
                text,
                ..
            },
        ) => delta
            .and_then(|d| d.get("thinking"))
            .and_then(Value::as_str)
            .map(|fragment| {
                text.push_str(fragment);
                (
                    model_item_id.clone(),
                    ModelBlockDelta::ReasoningText {
                        delta: fragment.to_string(),
                    },
                )
            }),
        ("signature_delta", ActiveMessagesBlock::Reasoning { signature, .. }) => {
            if let Some(fragment) = delta
                .and_then(|d| d.get("signature"))
                .and_then(Value::as_str)
            {
                signature.push_str(fragment);
            }
            None
        }
        ("text_delta", ActiveMessagesBlock::AssistantText { model_item_id }) => delta
            .and_then(|d| d.get("text"))
            .and_then(Value::as_str)
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
            .and_then(Value::as_str)
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
    reasoning_blocks: &mut Vec<(usize, ReasoningBlock)>,
    event: &Value,
) -> bool {
    let index = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let Some(active_block) = active_blocks.remove(&index) else {
        return true;
    };
    let model_item_id = match active_block {
        ActiveMessagesBlock::Reasoning {
            model_item_id,
            provider_item_id,
            text,
            signature,
        } => {
            reasoning_blocks.push((
                index,
                ReasoningBlock::Text(ReasoningTextBlock {
                    provider_item_id,
                    provider_index: Some(index as u32),
                    text,
                    signature: (!signature.is_empty()).then_some(signature),
                    format: Some("anthropic-claude-v1".to_string()),
                }),
            ));
            model_item_id
        }
        ActiveMessagesBlock::EncryptedReasoning {
            provider_item_id,
            data,
        } => {
            reasoning_blocks.push((
                index,
                ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
                    provider_item_id,
                    provider_index: Some(index as u32),
                    data,
                    format: Some("anthropic-claude-v1".to_string()),
                }),
            ));
            return true;
        }
        ActiveMessagesBlock::AssistantText { model_item_id }
        | ActiveMessagesBlock::ToolCall { model_item_id } => model_item_id,
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted { model_item_id }),
    )
    .await
}
