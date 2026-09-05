use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::UserActionResponse;

/// Untrusted typed content submitted by an Agent API caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentUserInput {
    pub content: Vec<AgentUserContent>,
}

/// Ordered User-authored content accepted by every Turn input API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentUserContent {
    Text {
        text: String,
    },
    SkillSelection {
        name: String,
        path: String,
    },
    File {
        file_id: String,
    },
    Audio {
        source: AgentMediaSource,
        mime_type: Option<String>,
    },
    Video {
        source: AgentMediaSource,
        mime_type: Option<String>,
    },
    UserActionResponse {
        action_id: String,
        response: UserActionResponse,
    },
}

/// Exact source of one submitted media content item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentMediaSource {
    Url { url: String },
    InlineBase64 { data: String },
}
