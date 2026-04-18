use crate::r#trait::ToolOutput;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureKind {
    ValidationFailure,
    PermissionRejection,
    GovernanceRejection,
    StaleWriteFailure,
    DeferredToolUnavailable,
    RuntimeFailure,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolRenderKind {
    Progress,
    Rejected,
    Error,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRenderMessage {
    pub kind: ToolRenderKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolPending {
    Approval { prompt: String, arguments: Value },
    Dynamic { arguments: Value },
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

    pub fn summary_message(&self, tool_name: &str) -> ToolRenderMessage {
        match self {
            Self::Function {
                success, content, ..
            } => {
                let text = if *success {
                    build_compact_summary(content, "completed")
                } else {
                    build_compact_summary(
                        self.error_message()
                            .as_deref()
                            .unwrap_or("tool execution failed"),
                        "failed",
                    )
                };
                ToolRenderMessage {
                    kind: if *success {
                        ToolRenderKind::Summary
                    } else {
                        ToolRenderKind::Error
                    },
                    text: format!("{tool_name}: {text}"),
                }
            }
            Self::Resource { uri, .. } => ToolRenderMessage {
                kind: ToolRenderKind::Summary,
                text: format!("{tool_name}: returned resource {uri}"),
            },
            Self::Mcp { content, is_error } => ToolRenderMessage {
                kind: if *is_error {
                    ToolRenderKind::Error
                } else {
                    ToolRenderKind::Summary
                },
                text: format!(
                    "{tool_name}: {}",
                    build_compact_summary(content, "completed")
                ),
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
        self.payload()
            .map(|payload| payload.is_error())
            .unwrap_or(false)
    }

    pub fn content(&self) -> &str {
        self.payload()
            .map(|payload| payload.content())
            .unwrap_or("")
    }

    pub fn render_progress_message(&self, tool_name: &str) -> Option<ToolRenderMessage> {
        match self {
            Self::Pending {
                pending: ToolPending::Approval { prompt, .. },
                ..
            } => Some(ToolRenderMessage {
                kind: ToolRenderKind::Progress,
                text: format!("{tool_name}: waiting for approval ({prompt})"),
            }),
            Self::Pending {
                pending: ToolPending::Dynamic { .. },
                ..
            } => Some(ToolRenderMessage {
                kind: ToolRenderKind::Progress,
                text: format!("{tool_name}: waiting for dynamic tool result"),
            }),
            Self::Executed { .. } => None,
        }
    }

    pub fn render_rejected_message(&self, tool_name: &str) -> Option<ToolRenderMessage> {
        let payload = self.payload()?;
        let error = payload.error_message()?;
        let failure_kind = classify_error_kind(&error);
        matches!(
            failure_kind,
            ToolFailureKind::PermissionRejection
                | ToolFailureKind::GovernanceRejection
                | ToolFailureKind::DeferredToolUnavailable
        )
        .then(|| ToolRenderMessage {
            kind: ToolRenderKind::Rejected,
            text: format!("{tool_name}: {error}"),
        })
    }

    pub fn render_error_message(&self, tool_name: &str) -> Option<ToolRenderMessage> {
        let payload = self.payload()?;
        let error = payload.error_message()?;
        (!matches!(
            classify_error_kind(&error),
            ToolFailureKind::PermissionRejection
                | ToolFailureKind::GovernanceRejection
                | ToolFailureKind::DeferredToolUnavailable
        ))
        .then(|| ToolRenderMessage {
            kind: ToolRenderKind::Error,
            text: format!("{tool_name}: {error}"),
        })
    }

    pub fn summarize_for_model(&self, tool_name: &str) -> ToolRenderMessage {
        self.payload()
            .map(|payload| payload.summary_message(tool_name))
            .or_else(|| self.render_progress_message(tool_name))
            .unwrap_or(ToolRenderMessage {
                kind: ToolRenderKind::Summary,
                text: format!("{tool_name}: no result"),
            })
    }

    pub fn summarize_for_transcript(&self, tool_name: &str) -> String {
        self.summarize_for_model(tool_name).text
    }
}

impl From<ToolOutput> for ToolOutputPayload {
    fn from(value: ToolOutput) -> Self {
        let structured_success = serde_json::from_str::<Value>(&value.output).ok();
        let structured_error = value.error.as_ref().map(|error| {
            let error_kind = classify_error_kind(error);
            serde_json::json!({
                "error": error,
                "error_kind": error_kind,
                "retryable": matches!(
                    error_kind,
                    ToolFailureKind::ValidationFailure
                        | ToolFailureKind::RuntimeFailure
                        | ToolFailureKind::StaleWriteFailure
                ),
                "suggested_action": suggested_action_for_error(error_kind),
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

fn classify_error_kind(error: &str) -> ToolFailureKind {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("invalid") || normalized.contains("required") {
        ToolFailureKind::ValidationFailure
    } else if normalized.contains("authorization denied")
        || normalized.contains("permission denied")
    {
        ToolFailureKind::PermissionRejection
    } else if normalized.contains("approval") || normalized.contains("governance") {
        ToolFailureKind::GovernanceRejection
    } else if normalized.contains("stale") {
        ToolFailureKind::StaleWriteFailure
    } else if normalized.contains("deferred") || normalized.contains("not yet visible") {
        ToolFailureKind::DeferredToolUnavailable
    } else {
        ToolFailureKind::RuntimeFailure
    }
}

fn suggested_action_for_error(kind: ToolFailureKind) -> &'static str {
    match kind {
        ToolFailureKind::ValidationFailure => "fix the arguments and retry",
        ToolFailureKind::PermissionRejection | ToolFailureKind::GovernanceRejection => {
            "request approval or choose a less privileged tool"
        }
        ToolFailureKind::StaleWriteFailure => "re-read the target file before retrying",
        ToolFailureKind::DeferredToolUnavailable => {
            "use ToolSearch or start a new turn to activate the deferred tool"
        }
        ToolFailureKind::RuntimeFailure => "narrow scope, retry, or choose a different tool",
    }
}

fn build_compact_summary(content: &str, fallback: &str) -> String {
    let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return fallback.to_string();
    }
    let mut chars = compact.chars();
    let preview = chars.by_ref().take(120).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}
