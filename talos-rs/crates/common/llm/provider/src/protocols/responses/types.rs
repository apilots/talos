use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesRequest {
    pub model: String,
    pub input: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponsesTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ResponsesToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponsesReasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesTool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ResponsesToolChoice {
    Simple(String),
    Named { r#type: String, name: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesReasoning {
    pub effort: String,
}

/// Provider-owned options applied by the shared Responses protocol encoder.
#[derive(Debug, Clone, Default)]
pub struct ResponsesRequestOptions {
    pub reasoning: Option<ResponsesReasoning>,
    pub include: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub output_text: Option<String>,
    #[serde(default)]
    pub output: Vec<Value>,
    #[serde(default)]
    pub usage: Option<ResponsesUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
    #[serde(default)]
    pub input_tokens_details: Option<ResponsesInputTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesInputTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ResponsesModelBlocks {
    pub provider_name: String,
    pub item_prefix: String,
}

impl ResponsesModelBlocks {
    pub fn openai() -> Self {
        Self {
            provider_name: "OpenAI".to_string(),
            item_prefix: "openai".to_string(),
        }
    }

    pub fn openrouter() -> Self {
        Self {
            provider_name: "OpenRouter".to_string(),
            item_prefix: "openrouter".to_string(),
        }
    }
}
