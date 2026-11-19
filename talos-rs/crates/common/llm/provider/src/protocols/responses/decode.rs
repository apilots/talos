use super::{
    item_type,
    terminal::{compatible_finish_reason, official_finish_reason},
    OfficialResponsesResponse, ResponsesResponse, ResponsesUsage,
};
use crate::{
    ChatResponse, FunctionCall, ReasoningBlock, ReasoningContinuation, ReasoningEncryptedBlock,
    ReasoningOutput, ReasoningSummaryBlock, ReasoningTextBlock, Usage,
};
use serde_json::Value;
use talos_error::TalosResult;

const REASONING_FORMAT: &str = "openai-responses-v1";

pub fn usage(usage: Option<ResponsesUsage>) -> Usage {
    let Some(usage) = usage else {
        return Usage::default();
    };
    let input_details = usage.input_tokens_details.as_ref();
    Usage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage.total_tokens,
        reasoning_tokens: usage
            .output_tokens_details
            .and_then(|details| details.reasoning_tokens),
        cache_creation_input_tokens: input_details.and_then(|details| details.cache_write_tokens),
        cache_read_input_tokens: input_details.and_then(|details| details.cached_tokens),
    }
}

pub fn chat_response(response: ResponsesResponse) -> ChatResponse {
    let finish_reason = compatible_finish_reason(response.status.as_deref());
    build_chat_response(response, finish_reason)
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
    Ok(build_chat_response(response.response, finish_reason))
}

fn build_chat_response(response: ResponsesResponse, finish_reason: Option<String>) -> ChatResponse {
    let (content, reasoning, tool_calls) = response_content(&response);
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
            "message" if !has_output_text => text.extend(text_from_message_item(item)),
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

fn text_from_message_item(item: &Value) -> Vec<String> {
    item["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(message_part_text)
        .map(str::to_owned)
        .collect()
}

pub(super) fn message_part_text(part: &Value) -> Option<&str> {
    let field = match item_type(part) {
        "output_text" | "text" => "text",
        "refusal" => "refusal",
        _ => return None,
    };
    part[field].as_str()
}

pub(super) enum ReasoningPart<'a> {
    Summary(&'a str),
    Text(&'a str),
    Encrypted(&'a str),
}

impl ReasoningPart<'_> {
    pub fn into_block(self, id: Option<&str>) -> ReasoningBlock {
        let provider_item_id = id.map(str::to_owned);
        let format = Some(REASONING_FORMAT.to_owned());
        match self {
            Self::Summary(text) => ReasoningBlock::Summary(ReasoningSummaryBlock {
                provider_item_id,
                provider_index: None,
                text: text.to_owned(),
                format,
            }),
            Self::Text(text) => ReasoningBlock::Text(ReasoningTextBlock {
                provider_item_id,
                provider_index: None,
                text: text.to_owned(),
                signature: None,
                format,
            }),
            Self::Encrypted(data) => ReasoningBlock::Encrypted(ReasoningEncryptedBlock {
                provider_item_id,
                provider_index: None,
                data: data.to_owned(),
                format,
            }),
        }
    }
}

pub(super) fn reasoning_part_text(part: &Value) -> Option<&str> {
    part["text"].as_str()
}

fn reasoning_from_item(item: &Value) -> (Vec<String>, Vec<ReasoningBlock>) {
    let id = item["id"].as_str();
    let summaries = item["summary"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(reasoning_part_text)
        .map(|text| (text, ReasoningPart::Summary(text)));
    let content = item["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(reasoning_part_text)
        .map(|text| (text, ReasoningPart::Text(text)));
    let (display_text, mut blocks): (Vec<_>, Vec<_>) = summaries
        .chain(content)
        .map(|(text, part)| (text.to_owned(), part.into_block(id)))
        .unzip();
    if let Some(data) = item["encrypted_content"].as_str() {
        blocks.push(ReasoningPart::Encrypted(data).into_block(id));
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
