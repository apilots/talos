use super::{
    item_type,
    terminal::{compatible_finish_reason, official_finish_reason},
    OfficialResponsesResponse, ResponsesContract, ResponsesResponse, ResponsesUsage,
};
use crate::{
    ChatResponse, FunctionCall, ReasoningBlock, ReasoningContinuation, ReasoningEncryptedBlock,
    ReasoningOutput, ReasoningSummaryBlock, ReasoningTextBlock, Usage,
};
use serde_json::Value;
use talos_error::TalosResult;

pub fn usage(usage: Option<ResponsesUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let input_details = usage.input_tokens_details.as_ref();
    Usage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        cache_creation_input_tokens: input_details.and_then(|details| details.cache_write_tokens),
        cache_read_input_tokens: input_details.and_then(|details| details.cached_tokens),
    }
}

pub fn chat_response(response: ResponsesResponse) -> ChatResponse {
    let finish_reason = compatible_finish_reason(response.status.as_deref());
    build_chat_response(response, finish_reason, ResponsesContract::Compatible)
}

/// Maps the official API's explicit terminal states before returning the reply.
pub fn official_chat_response(response: OfficialResponsesResponse) -> TalosResult<ChatResponse> {
    let finish_reason = official_finish_reason(
        response.response.status.as_deref(),
        response
            .error
            .as_ref()
            .and_then(|error| error.code.as_deref()),
        response
            .incomplete_details
            .as_ref()
            .and_then(|details| details.reason.as_deref()),
        response
            .response
            .output
            .iter()
            .any(|item| item_type(item) == "function_call"),
    )?;
    Ok(build_chat_response(
        response.response,
        finish_reason,
        ResponsesContract::Official,
    ))
}

fn build_chat_response(
    response: ResponsesResponse,
    finish_reason: Option<String>,
    contract: ResponsesContract,
) -> ChatResponse {
    let (content, reasoning, tool_calls) = response_content_with_contract(&response, contract);
    ChatResponse {
        content,
        reasoning,
        model: response.model.unwrap_or_default(),
        usage: usage(response.usage),
        finish_reason,
        tool_calls,
    }
}

pub fn response_content(
    response: &ResponsesResponse,
) -> (String, ReasoningOutput, Option<Vec<FunctionCall>>) {
    response_content_with_contract(response, ResponsesContract::Compatible)
}

fn response_content_with_contract(
    response: &ResponsesResponse,
    contract: ResponsesContract,
) -> (String, ReasoningOutput, Option<Vec<FunctionCall>>) {
    let mut text = Vec::new();
    let mut display_text = Vec::new();
    let mut continuation = Vec::new();
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
            "message" if !has_output_text => text.extend(text_from_message_item(item, contract)),
            "reasoning" => {
                let (display, blocks) = reasoning_from_item(item);
                display_text.extend(display);
                continuation.extend(blocks);
            }
            "function_call" => calls.push(function_call(item)),
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

fn text_from_message_item(item: &Value, contract: ResponsesContract) -> Vec<String> {
    let Some(content) = item.get("content").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| {
            let field = match item_type(block) {
                "output_text" | "text" => "text",
                "refusal" if contract == ResponsesContract::Official => "refusal",
                _ => return None,
            };
            block.get(field).and_then(Value::as_str).map(str::to_string)
        })
        .collect()
}

pub(super) fn reasoning_from_item(item: &Value) -> (Vec<String>, Vec<ReasoningBlock>) {
    let provider_item_id = item.get("id").and_then(Value::as_str).map(str::to_string);
    let mut display_text = Vec::new();
    let mut blocks = Vec::new();

    if let Some(items) = item.get("summary").and_then(|v| v.as_array()) {
        for block in items {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                display_text.push(text.to_string());
                blocks.push(ReasoningBlock::Summary(ReasoningSummaryBlock {
                    provider_item_id: provider_item_id.clone(),
                    provider_index: None,
                    text: text.to_string(),
                    format: Some("openai-responses-v1".to_string()),
                }));
            }
        }
    }
    if let Some(items) = item.get("content").and_then(|v| v.as_array()) {
        for block in items {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                display_text.push(text.to_string());
                blocks.push(ReasoningBlock::Text(ReasoningTextBlock {
                    provider_item_id: provider_item_id.clone(),
                    provider_index: None,
                    text: text.to_string(),
                    signature: None,
                    format: Some("openai-responses-v1".to_string()),
                }));
            }
        }
    }
    if let Some(data) = item.get("encrypted_content").and_then(Value::as_str) {
        blocks.push(ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
            provider_item_id,
            provider_index: None,
            data: data.to_string(),
            format: Some("openai-responses-v1".to_string()),
        }));
    }

    (display_text, blocks)
}

fn function_call(item: &Value) -> FunctionCall {
    FunctionCall::from_response(
        item.get("call_id")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        item.get("name").and_then(Value::as_str).unwrap_or_default(),
        item.get("arguments").cloned().unwrap_or(Value::Null),
    )
}
