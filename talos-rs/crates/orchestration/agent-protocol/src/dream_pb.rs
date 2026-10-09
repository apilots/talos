//! Projection of the existing Dream formation model callback's consumed fields.

use agent_pb as pb;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatResponse, ReasoningOutput, Role, Usage};

pub fn encode_prompt(messages: Vec<ChatMessage>) -> TalosResult<pb::DreamMemoryModelPrompt> {
    if messages.len() != 2
        || messages[0].role != Role::System
        || messages[1].role != Role::User
        || messages.iter().any(|message| {
            message.attachments.is_some()
                || !message.image_file_ids.is_empty()
                || message.name.is_some()
                || message.tool_call_id.is_some()
                || message.tool_calls.is_some()
                || message.reasoning_continuation.is_some()
                || message.cache_control.is_some()
        })
    {
        return Err(TalosError::internal("Dream Memory model prompt is invalid"));
    }
    let mut messages = messages.into_iter();
    Ok(pb::DreamMemoryModelPrompt {
        system: messages.next().expect("checked").content,
        user: messages.next().expect("checked").content,
    })
}

pub fn decode_prompt(prompt: pb::DreamMemoryModelPrompt) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(prompt.system),
        ChatMessage::user(prompt.user),
    ]
}

pub fn encode_reply(response: ChatResponse) -> pb::DreamMemoryModelReply {
    pb::DreamMemoryModelReply {
        content: response.content,
        usage: Some(pb::TokenUsage {
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
            total_tokens: response.usage.total_tokens,
            cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
            cache_read_input_tokens: response.usage.cache_read_input_tokens,
        }),
    }
}

pub fn decode_reply(response: pb::DreamMemoryModelReply) -> TalosResult<ChatResponse> {
    let usage = response
        .usage
        .ok_or_else(|| TalosError::internal("Dream Memory model usage is missing"))?;
    Ok(ChatResponse {
        content: response.content,
        reasoning: ReasoningOutput::default(),
        model: String::new(),
        finish_reason: None,
        tool_calls: None,
        usage: Usage {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
        },
    })
}
