use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::r#trait::ToolOutput;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolPending {
    Approval {
        prompt: String,
        arguments: Value,
    },
    Dynamic {
        arguments: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolOutputPayload {
    Function {
        content: String,
        success: bool,
        structured_content: Option<Value>,
    },
    Resource {
        uri: String,
        mime_type: Option<String>,
        content: String,
    },
    Mcp {
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolDispatchOutcome {
    Executed {
        payload: ToolOutputPayload,
    },
    Pending {
        tool_name: String,
        pending: ToolPending,
    },
}

impl ToolOutputPayload {
    pub fn is_error(&self) -> bool {
        match self {
            Self::Function { success, .. } => !success,
            Self::Resource { .. } => false,
            Self::Mcp { is_error, .. } => *is_error,
        }
    }

    pub fn content(&self) -> &str {
        match self {
            Self::Function { content, .. } => content,
            Self::Resource { content, .. } => content,
            Self::Mcp { content, .. } => content,
        }
    }

    pub fn structured_content(&self) -> Option<&Value> {
        match self {
            Self::Function {
                structured_content, ..
            } => structured_content.as_ref(),
            Self::Resource { .. } | Self::Mcp { .. } => None,
        }
    }

    pub fn error_message(&self) -> Option<String> {
        match self {
            Self::Function {
                content,
                success,
                structured_content,
            } => {
                if *success {
                    return None;
                }
                extract_error_message(structured_content.as_ref()).or_else(|| {
                    if content.trim().is_empty() {
                        None
                    } else {
                        Some(content.clone())
                    }
                })
            }
            Self::Resource { .. } => None,
            Self::Mcp { content, is_error } => {
                if *is_error {
                    Some(content.clone())
                } else {
                    None
                }
            }
        }
    }

    pub fn into_legacy_output(self) -> ToolOutput {
        match self {
            Self::Function {
                content,
                success,
                structured_content,
            } => ToolOutput {
                success,
                output: content,
                error: if success {
                    None
                } else {
                    extract_error_message(structured_content.as_ref())
                },
            },
            Self::Resource { content, .. } => ToolOutput {
                success: true,
                output: content,
                error: None,
            },
            Self::Mcp { content, is_error } => ToolOutput {
                success: !is_error,
                output: if is_error {
                    String::new()
                } else {
                    content.clone()
                },
                error: if is_error { Some(content) } else { None },
            },
        }
    }
}

impl ToolDispatchOutcome {
    pub fn executed(payload: ToolOutputPayload) -> Self {
        Self::Executed { payload }
    }

    pub fn pending_approval(
        tool_name: impl Into<String>,
        prompt: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self::Pending {
            tool_name: tool_name.into(),
            pending: ToolPending::Approval {
                prompt: prompt.into(),
                arguments,
            },
        }
    }

    pub fn pending_dynamic(tool_name: impl Into<String>, arguments: Value) -> Self {
        Self::Pending {
            tool_name: tool_name.into(),
            pending: ToolPending::Dynamic { arguments },
        }
    }

    pub fn payload(&self) -> Option<&ToolOutputPayload> {
        match self {
            Self::Executed { payload } => Some(payload),
            Self::Pending { .. } => None,
        }
    }

    pub fn pending(&self) -> Option<&ToolPending> {
        match self {
            Self::Executed { .. } => None,
            Self::Pending { pending, .. } => Some(pending),
        }
    }

    pub fn is_error(&self) -> bool {
        self.payload().map(|payload| payload.is_error()).unwrap_or(false)
    }

    pub fn content(&self) -> &str {
        self.payload().map(|payload| payload.content()).unwrap_or("")
    }

    pub fn into_legacy_output(self) -> ToolOutput {
        match self {
            Self::Executed { payload } => payload.into_legacy_output(),
            Self::Pending { pending, .. } => match pending {
                ToolPending::Approval { prompt, .. } => ToolOutput {
                    success: false,
                    output: String::new(),
                    error: Some(prompt),
                },
                ToolPending::Dynamic { .. } => ToolOutput {
                    success: false,
                    output: String::new(),
                    error: Some("dynamic tool response is pending".to_string()),
                },
            },
        }
    }
}

impl From<ToolOutput> for ToolOutputPayload {
    fn from(value: ToolOutput) -> Self {
        let structured_success = serde_json::from_str::<Value>(&value.output).ok();
        let structured_error = value.error.as_ref().map(|error| {
            serde_json::json!({
                "error": error,
            })
        });

        Self::Function {
            content: value.output,
            success: value.success,
            structured_content: if value.success {
                structured_success
            } else {
                structured_error
            },
        }
    }
}

fn extract_error_message(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|payload| payload.get("error"))
        .and_then(Value::as_str)
        .map(str::to_string)
}
