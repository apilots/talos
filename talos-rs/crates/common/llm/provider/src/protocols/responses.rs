use crate::{ChatMessage, ChatRequest, Role, ToolChoice, ToolDefinition};
use serde_json::Value;
use talos_error::TalosResult;

mod attachments;
mod decode;
mod reasoning;
mod stream;
mod terminal;
mod types;

pub use decode::{chat_response, official_chat_response, response_content, usage};
pub use stream::{model_blocks, model_blocks_with_contract, stream, stream_with_contract};
pub use types::*;

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

fn item_type(item: &Value) -> &str {
    item.get("type").and_then(Value::as_str).unwrap_or("")
}

fn event_type(event: &Value) -> &str {
    event.get("type").and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
