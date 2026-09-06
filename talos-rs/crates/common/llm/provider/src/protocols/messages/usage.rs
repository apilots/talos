use serde::Deserialize;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::{ChatResponse, Usage};

use super::{response_content, MessagesResponse};

#[derive(Debug, Clone, Deserialize)]
pub struct MessagesUsage {
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

pub fn chat_response(response: MessagesResponse) -> TalosResult<ChatResponse> {
    let (content, reasoning, tool_calls) = response_content(&response.content);
    Ok(ChatResponse {
        content,
        reasoning,
        model: response.model,
        usage: usage_from_messages(&response.usage, response.usage.output_tokens)?,
        finish_reason: response.stop_reason,
        tool_calls,
    })
}

pub(super) fn usage_from_messages(usage: &MessagesUsage, output_tokens: u32) -> TalosResult<Usage> {
    let prompt_tokens = usage
        .input_tokens
        .checked_add(usage.cache_creation_input_tokens.unwrap_or_default())
        .and_then(|value| value.checked_add(usage.cache_read_input_tokens.unwrap_or_default()))
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Messages input token usage overflow".to_string(),
            )
        })?;
    let total_tokens = prompt_tokens.checked_add(output_tokens).ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Messages total token usage overflow".to_string(),
        )
    })?;
    Ok(Usage {
        prompt_tokens,
        completion_tokens: output_tokens,
        total_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        cache_read_input_tokens: usage.cache_read_input_tokens,
    })
}

pub(super) fn stream_input_usage(event: &Value) -> TalosResult<Option<MessagesUsage>> {
    let Some(usage) = event
        .get("message")
        .and_then(|message| message.get("usage"))
    else {
        return Ok(None);
    };
    serde_json::from_value(usage.clone())
        .map(Some)
        .map_err(|error| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!("Messages stream usage is invalid: {error}"),
            )
        })
}
