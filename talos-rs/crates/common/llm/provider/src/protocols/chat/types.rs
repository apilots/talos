use crate::{CacheControl, ReasoningBlock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionMessage {
    pub role: String,
    pub content: Option<ChatCompletionMessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ChatCompletionToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<Vec<ChatCompletionReasoningDetail>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum ChatCompletionReasoningDetail {
    #[serde(rename = "reasoning.text")]
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    #[serde(rename = "reasoning.summary")]
    Summary {
        summary: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
    #[serde(rename = "reasoning.encrypted")]
    Encrypted {
        data: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
    },
}

impl From<ReasoningBlock> for ChatCompletionReasoningDetail {
    fn from(block: ReasoningBlock) -> Self {
        match block {
            ReasoningBlock::Text(block) => Self::Text {
                text: block.text,
                signature: block.signature,
                id: block.provider_item_id,
                format: block.format,
                index: block.provider_index,
            },
            ReasoningBlock::Summary(block) => Self::Summary {
                summary: block.text,
                id: block.provider_item_id,
                format: block.format,
                index: block.provider_index,
            },
            ReasoningBlock::Encrypted(block) => Self::Encrypted {
                data: block.data,
                id: block.provider_item_id,
                format: block.format,
                index: block.provider_index,
            },
        }
    }
}

impl ChatCompletionReasoningDetail {
    pub fn into_reasoning_block(self) -> ReasoningBlock {
        match self {
            Self::Text {
                text,
                signature,
                id,
                format,
                index,
            } => ReasoningBlock::Text(crate::ReasoningTextBlock {
                provider_item_id: id,
                provider_index: index,
                text,
                signature,
                format,
            }),
            Self::Summary {
                summary,
                id,
                format,
                index,
            } => ReasoningBlock::Summary(crate::ReasoningSummaryBlock {
                provider_item_id: id,
                provider_index: index,
                text: summary,
                format,
            }),
            Self::Encrypted {
                data,
                id,
                format,
                index,
            } => ReasoningBlock::Encrypted(crate::ReasoningEncryptedBlock {
                provider_item_id: id,
                provider_index: index,
                data,
                format,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ChatCompletionMessageContent {
    Text(String),
    Blocks(Vec<ChatCompletionContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCompletionContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionTool {
    pub r#type: String,
    pub function: ChatCompletionFunctionDefinition,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionFunctionDefinition {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionToolCall {
    pub id: String,
    pub r#type: String,
    pub function: ChatCompletionFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ChatCompletionToolChoice {
    Simple(String),
    Named {
        r#type: String,
        function: ChatCompletionToolChoiceFunction,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionToolChoiceFunction {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatCompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ChatCompletionTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ChatCompletionToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ChatCompletionReasoningConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ChatCompletionThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<ChatCompletionStreamOptions>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatCompletionReasoningConfig {
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatCompletionThinkingConfig {
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionStreamOptions {
    pub include_usage: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ChatCompletionRequestOptions {
    pub cache_control: bool,
    pub reasoning: Option<ChatCompletionReasoningConfig>,
    pub reasoning_effort: Option<String>,
    pub thinking: Option<ChatCompletionThinkingConfig>,
    pub enable_thinking: Option<bool>,
    pub thinking_budget: Option<u32>,
    pub reasoning_continuation: ChatReasoningContinuationFormat,
}

/// Selects the provider-specific assistant field used for reasoning replay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChatReasoningContinuationFormat {
    #[default]
    None,
    Reasoning,
    ReasoningContent,
    ReasoningDetails,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub object: Option<String>,
    #[serde(default)]
    pub created: Option<u64>,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    #[serde(default)]
    pub usage: Option<ChatCompletionUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionChoice {
    #[serde(default)]
    pub index: Option<u32>,
    pub message: ChatCompletionResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponseMessage {
    #[serde(default)]
    pub content: Option<ChatCompletionMessageContent>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ChatCompletionToolCall>>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub reasoning_details: Option<Vec<ChatCompletionReasoningDetail>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<ChatCompletionPromptTokensDetails>,
    #[serde(default)]
    pub cache_creation: Option<ChatCompletionCacheCreation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionPromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionCacheCreation {
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub ephemeral_5m_input_tokens: Option<u32>,
}
