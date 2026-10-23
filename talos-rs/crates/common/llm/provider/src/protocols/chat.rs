use crate::{
    ChatChunk, ChatMessage, ChatRequest, ChatResponse, ChatStream, FunctionCall,
    ModelBlockCompleted, ModelBlockDelta, ModelBlockKind, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, ModelOutputPhase, ModelToolCallStart, ReasoningContinuation,
    ReasoningEffort, ReasoningOutput, Role, SseLineDecoder, ToolChoice, Usage,
};
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::mpsc;

mod content;
mod reasoning;
mod tool_definitions;
mod types;

#[cfg(test)]
#[path = "chat/chat_tests.rs"]
mod tests;

pub use content::content_text;
use reasoning::{reasoning_fields, response_reasoning};
pub use tool_definitions::tools;
pub use types::*;

#[derive(Debug, Default)]
struct ModelBlockState {
    reasoning_started: HashMap<usize, bool>,
    reasoning_segment: HashMap<usize, usize>,
    assistant_started: HashMap<usize, bool>,
    assistant_segment: HashMap<usize, usize>,
    tool_started: HashMap<usize, bool>,
    reasoning_text: String,
    reasoning_details: Vec<ChatCompletionReasoningDetail>,
    reasoning_completed: bool,
}

#[derive(Debug, Default)]
struct ModelToolAcc {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
    emitted_arguments_len: usize,
}

/// Normalize leading plain instructions for compatible Chat templates without changing history boundaries.
pub fn merge_initial_system_messages(encoded: &mut ChatCompletionRequest) {
    // Compatible Chat templates commonly accept only one initial system message.
    // Merge only leading plain instructions; preserve every history boundary.
    let count = encoded
        .messages
        .iter()
        .take_while(|message| {
            message.role == "system"
                && message.name.is_none()
                && matches!(message.content, Some(ChatCompletionMessageContent::Text(_)))
        })
        .count();
    if count > 1 {
        let additional = encoded.messages.drain(1..count).collect::<Vec<_>>();
        if let Some(ChatCompletionMessageContent::Text(first)) = &mut encoded.messages[0].content {
            for message in additional {
                if let Some(ChatCompletionMessageContent::Text(text)) = message.content {
                    first.push_str("\n\n");
                    first.push_str(&text);
                }
            }
        }
    }
}

pub fn request(
    chat_request: ChatRequest,
    stream: bool,
    options: ChatCompletionRequestOptions,
) -> TalosResult<ChatCompletionRequest> {
    Ok(ChatCompletionRequest {
        model: chat_request.model,
        messages: messages(
            chat_request.messages,
            options.cache_control,
            options.reasoning_continuation,
        )?,
        temperature: chat_request.temperature,
        max_tokens: chat_request.max_tokens,
        top_p: chat_request.top_p,
        stop: chat_request.stop,
        tools: tools(chat_request.tools),
        tool_choice: tool_choice(chat_request.tool_choice),
        parallel_tool_calls: chat_request.parallel_tool_calls,
        reasoning: options.reasoning,
        reasoning_effort: options.reasoning_effort,
        thinking: options.thinking,
        enable_thinking: options.enable_thinking,
        thinking_budget: options.thinking_budget,
        stream: Some(stream),
        stream_options: if stream {
            Some(ChatCompletionStreamOptions {
                include_usage: true,
            })
        } else {
            None
        },
    })
}

pub fn reasoning_effort(effort: ReasoningEffort) -> String {
    match effort {
        ReasoningEffort::Light => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    }
    .to_string()
}

pub fn messages(
    chat_messages: Vec<ChatMessage>,
    cache_control: bool,
    reasoning_continuation: ChatReasoningContinuationFormat,
) -> TalosResult<Vec<ChatCompletionMessage>> {
    chat_messages
        .into_iter()
        .map(|message| {
            message.validate_attachments()?;
            let role = match message.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            }
            .to_string();

            let tool_calls = message.tool_calls.map(|calls| {
                calls
                    .into_iter()
                    .map(|call| ChatCompletionToolCall {
                        id: call.id,
                        r#type: "function".to_string(),
                        function: ChatCompletionFunctionCall {
                            name: call.name,
                            arguments: call.arguments.to_string(),
                        },
                    })
                    .collect()
            });

            let content = content::message_content(
                message.content,
                message.cache_control,
                message.attachments,
                cache_control,
            );

            let (reasoning, reasoning_content, reasoning_details) =
                reasoning_fields(message.reasoning_continuation, reasoning_continuation)?;

            Ok(ChatCompletionMessage {
                role,
                content,
                name: message.name,
                tool_call_id: message.tool_call_id,
                tool_calls,
                reasoning,
                reasoning_content,
                reasoning_details,
            })
        })
        .collect()
}

pub fn tool_choice(choice: Option<ToolChoice>) -> Option<ChatCompletionToolChoice> {
    match choice {
        Some(ToolChoice::Auto) => Some(ChatCompletionToolChoice::Simple("auto".to_string())),
        Some(ToolChoice::Required) => {
            Some(ChatCompletionToolChoice::Simple("required".to_string()))
        }
        Some(ToolChoice::None) => Some(ChatCompletionToolChoice::Simple("none".to_string())),
        Some(ToolChoice::Named { name }) => Some(ChatCompletionToolChoice::Named {
            r#type: "function".to_string(),
            function: ChatCompletionToolChoiceFunction { name },
        }),
        None => None,
    }
}

pub fn tool_calls(tool_calls: Option<Vec<ChatCompletionToolCall>>) -> Option<Vec<FunctionCall>> {
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

pub fn usage(usage: ChatCompletionUsage) -> TalosResult<Usage> {
    Ok(Usage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        cache_creation_input_tokens: match usage.cache_creation.as_ref() {
            Some(creation) => cache_creation_tokens(creation)?,
            None => None,
        },
        cache_read_input_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens),
    })
}

pub fn chat_response(
    response: ChatCompletionResponse,
    provider_name: &str,
) -> TalosResult<ChatResponse> {
    let choice = response.choices.into_iter().next().ok_or_else(|| {
        TalosError::internal("No choices in response.")
            .context(format!("No choices in {provider_name} response"))
    })?;

    let usage_value = response.usage.unwrap_or(ChatCompletionUsage {
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        prompt_tokens_details: None,
        cache_creation: None,
    });

    let reasoning = response_reasoning(&choice.message);
    Ok(ChatResponse {
        content: content_text(choice.message.content, provider_name)?,
        reasoning,
        model: response.model,
        usage: usage(usage_value)?,
        finish_reason: choice.finish_reason,
        tool_calls: tool_calls(choice.message.tool_calls),
    })
}

pub fn reasoning_delta(delta: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = delta.get(*key) {
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

fn reasoning_detail_display(detail: &ChatCompletionReasoningDetail) -> Option<String> {
    match detail {
        ChatCompletionReasoningDetail::Text { text, .. } => {
            (!text.is_empty()).then(|| text.clone())
        }
        ChatCompletionReasoningDetail::Summary { summary, .. } => {
            (!summary.is_empty()).then(|| summary.clone())
        }
        ChatCompletionReasoningDetail::Encrypted { .. } => None,
    }
}

fn plain_reasoning_detail(text: String) -> ChatCompletionReasoningDetail {
    ChatCompletionReasoningDetail::Text {
        text,
        signature: None,
        id: None,
        format: None,
        index: None,
    }
}

fn reasoning_detail_index(detail: &ChatCompletionReasoningDetail) -> Option<u32> {
    match detail {
        ChatCompletionReasoningDetail::Text { index, .. }
        | ChatCompletionReasoningDetail::Summary { index, .. }
        | ChatCompletionReasoningDetail::Encrypted { index, .. } => *index,
    }
}

fn accumulate_reasoning_detail(
    details: &mut Vec<ChatCompletionReasoningDetail>,
    incoming: ChatCompletionReasoningDetail,
) -> TalosResult<()> {
    let Some(index) = reasoning_detail_index(&incoming) else {
        details.push(incoming);
        return Ok(());
    };
    let Some(existing) = details
        .iter_mut()
        .find(|detail| reasoning_detail_index(detail) == Some(index))
    else {
        details.push(incoming);
        return Ok(());
    };

    match (existing, incoming) {
        (
            ChatCompletionReasoningDetail::Text {
                text,
                signature,
                id,
                format,
                ..
            },
            ChatCompletionReasoningDetail::Text {
                text: incoming_text,
                signature: incoming_signature,
                id: incoming_id,
                format: incoming_format,
                ..
            },
        ) => {
            text.push_str(&incoming_text);
            merge_reasoning_metadata(signature, incoming_signature, "signature", index)?;
            merge_reasoning_metadata(id, incoming_id, "id", index)?;
            merge_reasoning_metadata(format, incoming_format, "format", index)
        }
        (
            ChatCompletionReasoningDetail::Summary {
                summary,
                id,
                format,
                ..
            },
            ChatCompletionReasoningDetail::Summary {
                summary: incoming_summary,
                id: incoming_id,
                format: incoming_format,
                ..
            },
        ) => {
            summary.push_str(&incoming_summary);
            merge_reasoning_metadata(id, incoming_id, "id", index)?;
            merge_reasoning_metadata(format, incoming_format, "format", index)
        }
        (
            ChatCompletionReasoningDetail::Encrypted {
                data, id, format, ..
            },
            ChatCompletionReasoningDetail::Encrypted {
                data: incoming_data,
                id: incoming_id,
                format: incoming_format,
                ..
            },
        ) => {
            data.push_str(&incoming_data);
            merge_reasoning_metadata(id, incoming_id, "id", index)?;
            merge_reasoning_metadata(format, incoming_format, "format", index)
        }
        _ => Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!("reasoning detail index {index} changed type across stream chunks"),
        )),
    }
}

fn merge_reasoning_metadata(
    current: &mut Option<String>,
    incoming: Option<String>,
    field: &str,
    index: u32,
) -> TalosResult<()> {
    match (current.as_ref(), incoming) {
        (Some(current), Some(incoming)) if current != &incoming => Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!("reasoning detail index {index} changed {field} across stream chunks"),
        )),
        (None, Some(incoming)) => {
            *current = Some(incoming);
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn stream(response: reqwest::Response, provider_name: impl Into<String>) -> ChatStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(100);
    let provider_name = provider_name.into();

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
                                    .send(Err({ let diagnostic = format!(
                                        "{provider_name} stream invalid SSE JSON: {error}"
                                    ); TalosError::internal_with_source("Model response stream invalid SSE JSON; technical details were withheld.", error).context(diagnostic) }))
                                    .await;
                                return;
                            }
                        };
                        if let Some(error) = event.get("error") {
                            let _ = tx
                                .send(Err(crate::error::stream_response_error(
                                    error.get("code").and_then(Value::as_str),
                                )))
                                .await;
                            return;
                        }
                        let choice = first_choice(&event);

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

                        if let Some(finish_reason) = finish_reason(choice) {
                            final_finish_reason = Some(finish_reason.to_string());
                        }
                    }
                }
                Err(error) => {
                    let _ = tx
                        .send(Err(TalosError::internal_with_source(
                            "Stream error; technical details were withheld.",
                            error,
                        )))
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
    options: ChatCompletionModelBlocks,
) -> ModelBlockStream {
    let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(100);

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        let mut decoder = SseLineDecoder::new();
        let mut state = ModelBlockState::default();
        let mut tool_acc: HashMap<usize, ModelToolAcc> = HashMap::new();
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
                            if !complete_open_model_blocks(&tx, &mut state, &options).await {
                                return;
                            }
                            if !complete_reasoning_output(&tx, &mut state).await {
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
                                    .send(Err({ let diagnostic = format!(
                                        "{} model block stream invalid SSE JSON: {error}",
                                        options.provider_name
                                    ); TalosError::internal_with_source("Model response model block stream invalid SSE JSON; technical details were withheld.", error).context(diagnostic) }))
                                    .await;
                                return;
                            }
                        };

                        if let Some(error) = event.get("error") {
                            let _ = tx
                                .send(Err(crate::error::stream_response_error(
                                    error.get("code").and_then(Value::as_str),
                                )))
                                .await;
                            return;
                        }
                        if let Some(parsed_usage) = event.get("usage").and_then(|value| {
                            serde_json::from_value::<ChatCompletionUsage>(value.clone()).ok()
                        }) {
                            match usage(parsed_usage) {
                                Ok(mapped_usage) => final_usage = Some(mapped_usage),
                                Err(err) => {
                                    let _ = tx.send(Err(err)).await;
                                    return;
                                }
                            }
                        }

                        let choice = first_choice(&event);
                        let choice_index = choice_index(choice);
                        let delta_object = choice.and_then(|c| c.get("delta"));

                        let details = (options.reasoning_format
                            != ChatReasoningStreamFormat::TextAliases)
                            .then(|| delta_object.and_then(|delta| delta.get("reasoning_details")))
                            .flatten()
                            .and_then(Value::as_array)
                            .filter(|details| !details.is_empty());
                        if let Some(details) = details {
                            // Auto may discover structured continuation after a plain
                            // prelude. Retain that text before the structured blocks.
                            if options.reasoning_format == ChatReasoningStreamFormat::Auto
                                && state.reasoning_details.is_empty()
                                && !state.reasoning_text.is_empty()
                            {
                                state
                                    .reasoning_details
                                    .push(plain_reasoning_detail(state.reasoning_text.clone()));
                            }
                            for raw_detail in details {
                                let detail =
                                    match serde_json::from_value::<ChatCompletionReasoningDetail>(
                                        raw_detail.clone(),
                                    ) {
                                        Ok(detail) => detail,
                                        Err(error) => {
                                            let _ = tx
                                                .send(Err(TalosError::new(
                                                    ErrorCode::InvalidRequest,
                                                    format!(
                                                        "{} reasoning detail is invalid: {error}",
                                                        options.provider_name
                                                    ),
                                                )))
                                                .await;
                                            return;
                                        }
                                    };
                                let display_delta = reasoning_detail_display(&detail);
                                if let Err(error) = accumulate_reasoning_detail(
                                    &mut state.reasoning_details,
                                    detail,
                                ) {
                                    let _ = tx.send(Err(error)).await;
                                    return;
                                }
                                if let Some(display_delta) = display_delta {
                                    state.reasoning_text.push_str(&display_delta);
                                    if !ensure_reasoning_model_block(
                                        &tx,
                                        &mut state,
                                        choice_index,
                                        &options,
                                    )
                                    .await
                                        || !send_model_block_event(
                                            &tx,
                                            ModelBlockStreamEvent::BlockDelta {
                                                model_item_id: current_reasoning_model_item_id(
                                                    &state,
                                                    choice_index,
                                                    &options,
                                                ),
                                                delta: ModelBlockDelta::ReasoningText {
                                                    delta: display_delta,
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

                        let read_aliases = match options.reasoning_format {
                            ChatReasoningStreamFormat::TextAliases => true,
                            ChatReasoningStreamFormat::StructuredDetails => false,
                            ChatReasoningStreamFormat::Auto => details.is_none(),
                        };
                        if read_aliases {
                            if let Some(reasoning_delta) = delta_object
                                .and_then(|delta| reasoning_delta(delta, options.reasoning_keys))
                            {
                                let should_emit = (!options.ignore_empty_text
                                    || !reasoning_delta.trim().is_empty())
                                    && (!options.ignore_reasoning_after_assistant
                                        || !has_seen_assistant_text(&state, choice_index));
                                if should_emit {
                                    if options.reasoning_format == ChatReasoningStreamFormat::Auto
                                        && !state.reasoning_details.is_empty()
                                    {
                                        state
                                            .reasoning_details
                                            .push(plain_reasoning_detail(reasoning_delta.clone()));
                                    }
                                    state.reasoning_text.push_str(&reasoning_delta);
                                    if !ensure_reasoning_model_block(
                                        &tx,
                                        &mut state,
                                        choice_index,
                                        &options,
                                    )
                                    .await
                                        || !send_model_block_event(
                                            &tx,
                                            ModelBlockStreamEvent::BlockDelta {
                                                model_item_id: current_reasoning_model_item_id(
                                                    &state,
                                                    choice_index,
                                                    &options,
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
                            }
                        }

                        if let Some(delta) = delta_object
                            .and_then(|d| d.get("content"))
                            .and_then(|t| t.as_str())
                        {
                            if (!options.ignore_empty_text || !delta.is_empty())
                                && (!ensure_assistant_model_block(
                                    &tx,
                                    &mut state,
                                    choice_index,
                                    &options,
                                )
                                .await
                                    || !send_model_block_event(
                                        &tx,
                                        ModelBlockStreamEvent::BlockDelta {
                                            model_item_id: current_assistant_model_item_id(
                                                &state,
                                                choice_index,
                                                &options,
                                            ),
                                            delta: ModelBlockDelta::AssistantText {
                                                delta: delta.to_string(),
                                            },
                                        },
                                    )
                                    .await)
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
                                        .filter(|value| !value.trim().is_empty())
                                        .map(str::to_string)
                                    {
                                        entry.id = Some(id);
                                    }
                                    if let Some(name) = tool_call_delta
                                        .get("function")
                                        .and_then(|f| f.get("name"))
                                        .and_then(|v| v.as_str())
                                        .filter(|value| !value.trim().is_empty())
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
                                let entry_has_name = tool_acc
                                    .get(&index)
                                    .and_then(|entry| entry.name.as_deref())
                                    .is_some();
                                should_start = should_start || entry_has_name;

                                if should_start {
                                    let entry = tool_acc.entry(index).or_default();
                                    if !ensure_tool_model_block(
                                        &tx, &mut state, index, entry, &options,
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                }

                                if let Some(arguments_delta) = arguments_delta {
                                    let entry = tool_acc.entry(index).or_default();
                                    entry.arguments.push_str(&arguments_delta);
                                }

                                let Some(entry) = tool_acc.get_mut(&index) else {
                                    continue;
                                };
                                if state.tool_started.get(&index) == Some(&true)
                                    && entry.emitted_arguments_len < entry.arguments.len()
                                {
                                    let delta =
                                        entry.arguments[entry.emitted_arguments_len..].to_string();
                                    entry.emitted_arguments_len = entry.arguments.len();
                                    if !send_model_block_event(
                                        &tx,
                                        ModelBlockStreamEvent::BlockDelta {
                                            model_item_id: tool_model_item_id(index, &options),
                                            delta: ModelBlockDelta::ToolArguments { delta },
                                        },
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                }
                            }
                        }

                        if let Some(finish_reason) = finish_reason(choice) {
                            final_finish_reason = Some(finish_reason.to_string());
                            if !complete_open_model_blocks(&tx, &mut state, &options).await {
                                return;
                            }
                            if !complete_reasoning_output(&tx, &mut state).await {
                                return;
                            }
                        }
                    }
                }
                Err(error) => {
                    let _ = tx
                        .send(Err(TalosError::internal_with_source(
                            "Stream error; technical details were withheld.",
                            error,
                        )))
                        .await;
                    return;
                }
            }
        }
    });

    ModelBlockStream::new(rx)
}

fn first_choice(event: &Value) -> Option<&Value> {
    event
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
}

fn choice_index(choice: Option<&Value>) -> usize {
    choice
        .and_then(|c| c.get("index"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

fn finish_reason(choice: Option<&Value>) -> Option<&str> {
    choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
}

fn cache_creation_tokens(creation: &ChatCompletionCacheCreation) -> TalosResult<Option<u32>> {
    match (
        creation.cache_creation_input_tokens,
        creation.ephemeral_5m_input_tokens,
    ) {
        (Some(cache_creation_input_tokens), Some(ephemeral_5m_input_tokens))
            if cache_creation_input_tokens != ephemeral_5m_input_tokens =>
        {
            Err(TalosError::new(ErrorCode::InvalidRequest, format!(
                "Bailian cache creation token fields disagree: cache_creation_input_tokens={} ephemeral_5m_input_tokens={}",
                cache_creation_input_tokens, ephemeral_5m_input_tokens
            )))
        }
        (Some(cache_creation_input_tokens), _) => Ok(Some(cache_creation_input_tokens)),
        (None, Some(ephemeral_5m_input_tokens)) => Ok(Some(ephemeral_5m_input_tokens)),
        (None, None) => Ok(None),
    }
}

fn reasoning_model_item_id(
    choice_index: usize,
    segment: usize,
    options: &ChatCompletionModelBlocks,
) -> String {
    if !options.segment_after_boundary || segment == 0 {
        format!("{}-choice-{choice_index}-reasoning", options.item_prefix)
    } else {
        format!(
            "{}-choice-{choice_index}-reasoning-{segment}",
            options.item_prefix
        )
    }
}

fn current_reasoning_model_item_id(
    state: &ModelBlockState,
    choice_index: usize,
    options: &ChatCompletionModelBlocks,
) -> String {
    let segment = state
        .reasoning_segment
        .get(&choice_index)
        .copied()
        .unwrap_or(0);
    reasoning_model_item_id(choice_index, segment, options)
}

fn assistant_model_item_id(
    choice_index: usize,
    segment: usize,
    options: &ChatCompletionModelBlocks,
) -> String {
    if !options.segment_after_boundary || segment == 0 {
        format!("{}-choice-{choice_index}-assistant", options.item_prefix)
    } else {
        format!(
            "{}-choice-{choice_index}-assistant-{segment}",
            options.item_prefix
        )
    }
}

fn current_assistant_model_item_id(
    state: &ModelBlockState,
    choice_index: usize,
    options: &ChatCompletionModelBlocks,
) -> String {
    let segment = state
        .assistant_segment
        .get(&choice_index)
        .copied()
        .unwrap_or(0);
    assistant_model_item_id(choice_index, segment, options)
}

fn has_seen_assistant_text(state: &ModelBlockState, choice_index: usize) -> bool {
    state.assistant_started.get(&choice_index) == Some(&true)
        || state
            .assistant_segment
            .get(&choice_index)
            .copied()
            .unwrap_or(0)
            > 0
}

fn tool_model_item_id(index: usize, options: &ChatCompletionModelBlocks) -> String {
    format!("{}-tool-{index}", options.item_prefix)
}

async fn send_model_block_event(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    event: ModelBlockStreamEvent,
) -> bool {
    tx.send(Ok(event)).await.is_ok()
}

async fn ensure_reasoning_model_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    state: &mut ModelBlockState,
    choice_index: usize,
    options: &ChatCompletionModelBlocks,
) -> bool {
    if state.reasoning_started.get(&choice_index) == Some(&true) {
        return true;
    }
    state.reasoning_started.insert(choice_index, true);
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id: current_reasoning_model_item_id(state, choice_index, options),
            provider_item_id: Some(format!("choice:{choice_index}:reasoning")),
            kind: ModelBlockKind::Reasoning,
            output_phase: ModelOutputPhase::Unknown,
            tool_call: None,
        }),
    )
    .await
}

async fn ensure_assistant_model_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    state: &mut ModelBlockState,
    choice_index: usize,
    options: &ChatCompletionModelBlocks,
) -> bool {
    if state.assistant_started.get(&choice_index) == Some(&true) {
        return true;
    }
    state.assistant_started.insert(choice_index, true);
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id: current_assistant_model_item_id(state, choice_index, options),
            provider_item_id: Some(format!("choice:{choice_index}:message")),
            kind: ModelBlockKind::AssistantText,
            output_phase: ModelOutputPhase::Unknown,
            tool_call: None,
        }),
    )
    .await
}

async fn ensure_tool_model_block(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    state: &mut ModelBlockState,
    index: usize,
    acc: &ModelToolAcc,
    options: &ChatCompletionModelBlocks,
) -> bool {
    if state.tool_started.get(&index) == Some(&true) {
        return true;
    }
    state.tool_started.insert(index, true);
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::BlockStarted(ModelBlockStarted {
            model_item_id: tool_model_item_id(index, options),
            provider_item_id: acc.id.clone(),
            kind: ModelBlockKind::ToolCall,
            output_phase: ModelOutputPhase::Unknown,
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
    state: &mut ModelBlockState,
    options: &ChatCompletionModelBlocks,
) -> bool {
    let reasoning_ids: Vec<_> = state
        .reasoning_started
        .iter()
        .filter_map(|(index, started)| started.then_some(*index))
        .collect();
    for index in reasoning_ids {
        let segment = state.reasoning_segment.get(&index).copied().unwrap_or(0);
        state.reasoning_started.insert(index, false);
        if !send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                model_item_id: reasoning_model_item_id(index, segment, options),
            }),
        )
        .await
        {
            return false;
        }
        if options.segment_after_boundary {
            state.reasoning_segment.insert(index, segment + 1);
        }
    }

    let assistant_ids: Vec<_> = state
        .assistant_started
        .iter()
        .filter_map(|(index, started)| started.then_some(*index))
        .collect();
    for index in assistant_ids {
        let segment = state.assistant_segment.get(&index).copied().unwrap_or(0);
        state.assistant_started.insert(index, false);
        if !send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                model_item_id: assistant_model_item_id(index, segment, options),
            }),
        )
        .await
        {
            return false;
        }
        if options.segment_after_boundary {
            state.assistant_segment.insert(index, segment + 1);
        }
    }

    let tool_ids: Vec<_> = state
        .tool_started
        .iter()
        .filter_map(|(index, started)| started.then_some(*index))
        .collect();
    for index in tool_ids {
        state.tool_started.insert(index, false);
        if !send_model_block_event(
            tx,
            ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                model_item_id: tool_model_item_id(index, options),
            }),
        )
        .await
        {
            return false;
        }
    }
    true
}

async fn complete_reasoning_output(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    state: &mut ModelBlockState,
) -> bool {
    if state.reasoning_completed
        || (state.reasoning_text.is_empty() && state.reasoning_details.is_empty())
    {
        return true;
    }
    state.reasoning_completed = true;
    let reasoning_text = state.reasoning_text.clone();
    let display_text = (!reasoning_text.is_empty()).then_some(reasoning_text.clone());
    let continuation = if state.reasoning_details.is_empty() {
        ReasoningContinuation::Text(reasoning_text)
    } else {
        ReasoningContinuation::Blocks(
            std::mem::take(&mut state.reasoning_details)
                .into_iter()
                .map(ChatCompletionReasoningDetail::into_reasoning_block)
                .collect(),
        )
    };
    send_model_block_event(
        tx,
        ModelBlockStreamEvent::ReasoningCompleted {
            output: ReasoningOutput {
                display_text: display_text.into_iter().collect(),
                continuation: Some(continuation),
            },
        },
    )
    .await
}
