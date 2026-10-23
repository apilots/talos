use super::usage::MessagesUsage;
use crate::CacheControl;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub output_config: Option<MessagesOutputConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessagesThinkingConfig {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessagesOutputConfig {
    pub effort: String,
}

/// Provider-owned options applied by the shared Messages protocol encoder.
#[derive(Debug, Clone, Default)]
pub struct MessagesRequestOptions {
    pub thinking: Option<MessagesThinkingConfig>,
    pub output_config: Option<MessagesOutputConfig>,
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
    pub signature: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessagesTextBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}
