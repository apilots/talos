use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use talos_error::{TalosError, TalosResult, TalosToolErrorKind};
use talos_types::talos::{JsonContentValue, TalosContentItem};
use talos_types::{QuestionParams, SandboxApprovalParams};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolRenderKind {
    Progress,
    Rejected,
    Error,
    Summary,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Success,
    Failure,
    Pending,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolRenderMessage {
    pub kind: ToolRenderKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolResultPolicyMeta {
    pub max_inline_bytes: Option<usize>,
    pub persisted_large_output: bool,
    pub preview_lines: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultError {
    pub message: String,
    pub error_kind: TalosToolErrorKind,
    pub retryable: bool,
    pub suggested_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultPendingMeta {
    pub pending_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NextTurnFailurePayload {
    pub error_kind: TalosToolErrorKind,
    pub retryable: bool,
    pub suggested_action: String,
    pub strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultEnvelope {
    pub status: ToolResultStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_items: Vec<TalosContentItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolResultError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<ToolResultPendingMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<ToolResultPolicyMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolPending {
    Dynamic {
        arguments: Value,
    },
    Question {
        params: QuestionParams,
    },
    SandboxApproval {
        params: SandboxApprovalParams,
        request_fingerprint: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolOutputPayload {
    Function {
        content: String,
        success: bool,
        structured_content: Option<Value>,
        content_items: Vec<TalosContentItem>,
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
    pub fn success_text(content: impl Into<String>) -> Self {
        let content = content.into();
        Self::Function {
            structured_content: Some(canonical_success_content(&content, None)),
            content_items: vec![TalosContentItem::Text {
                text: content.clone(),
            }],
            content,
            success: true,
        }
    }

    pub fn success_json<T>(value: &T, context: &str) -> TalosResult<Self>
    where
        T: Serialize,
    {
        let structured_content = serde_json::to_value(value).map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("{context}: {err}"),
            )
        })?;
        let content = serde_json::to_string(value).map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("{context}: {err}"),
            )
        })?;
        Ok(Self::Function {
            content,
            success: true,
            structured_content: Some(structured_content.clone()),
            content_items: vec![TalosContentItem::Json {
                value: JsonContentValue::from(structured_content),
            }],
        })
    }

    pub fn success_structured(value: Value) -> Self {
        Self::Function {
            content: value.to_string(),
            success: true,
            structured_content: Some(value.clone()),
            content_items: vec![TalosContentItem::Json {
                value: JsonContentValue::from(value),
            }],
        }
    }

    pub fn success_with_content_items(value: Value, content_items: Vec<TalosContentItem>) -> Self {
        Self::Function {
            content: value.to_string(),
            success: true,
            structured_content: Some(value),
            content_items,
        }
    }

    pub fn failure_with_content_items(value: Value, content_items: Vec<TalosContentItem>) -> Self {
        Self::Function {
            content: value.to_string(),
            success: false,
            structured_content: Some(value),
            content_items,
        }
    }

    pub fn failure_structured(
        content: impl Into<String>,
        structured_content: Option<Value>,
    ) -> Self {
        let content = content.into();
        Self::Function {
            content: content.clone(),
            success: false,
            structured_content,
            content_items: vec![TalosContentItem::Text { text: content }],
        }
    }

    pub fn validate_canonical_result(&self) -> TalosResult<()> {
        if let Self::Function {
            success,
            structured_content,
            content_items,
            ..
        } = self
        {
            if *success && content_items.is_empty() && structured_content.is_some() {
                return Err(TalosError::new(talos_error::ErrorCode::ToolExecutionFailed,
                    "canonical tool result requires content_items when structured_content is present"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

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
                ..
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
        let envelope = self.envelope();
        match envelope.status {
            ToolResultStatus::Success => match self {
                Self::Resource { uri, .. } => ToolRenderMessage {
                    kind: ToolRenderKind::Summary,
                    text: format!("{tool_name}: returned resource {uri}"),
                },
                _ => ToolRenderMessage {
                    kind: ToolRenderKind::Summary,
                    text: format!(
                        "{tool_name}: {}",
                        build_compact_summary(self.content(), "completed")
                    ),
                },
            },
            ToolResultStatus::Rejected => ToolRenderMessage {
                kind: ToolRenderKind::Rejected,
                text: format!(
                    "{tool_name}: {}",
                    envelope
                        .error
                        .as_ref()
                        .map(|item| item.message.as_str())
                        .unwrap_or("tool execution rejected")
                ),
            },
            ToolResultStatus::Failure => ToolRenderMessage {
                kind: ToolRenderKind::Error,
                text: format!(
                    "{tool_name}: {}",
                    envelope
                        .error
                        .as_ref()
                        .map(|item| item.message.as_str())
                        .unwrap_or("tool execution failed")
                ),
            },
            ToolResultStatus::Pending => ToolRenderMessage {
                kind: ToolRenderKind::Progress,
                text: format!("{tool_name}: waiting for tool result"),
            },
        }
    }

    pub fn envelope(&self) -> ToolResultEnvelope {
        match self {
            Self::Function {
                content,
                success,
                structured_content,
                content_items,
            } => {
                if *success {
                    ToolResultEnvelope {
                        status: ToolResultStatus::Success,
                        structured_content: Some(canonical_success_content(
                            content,
                            structured_content.as_ref(),
                        )),
                        content_items: content_items.clone(),
                        compact_summary: Some(build_content_items_summary(
                            content_items,
                            content,
                            "completed",
                        )),
                        error: None,
                        pending: None,
                        policy: None,
                    }
                } else {
                    let error = if let Some(structured) = structured_content.as_ref() {
                        structured_error_from_value(structured).unwrap_or_else(|| {
                            ToolResultError::from_message(if content.trim().is_empty() {
                                "tool execution failed"
                            } else {
                                content
                            })
                        })
                    } else {
                        ToolResultError::from_message(if content.trim().is_empty() {
                            "tool execution failed"
                        } else {
                            content
                        })
                    };
                    ToolResultEnvelope {
                        status: status_for_error_kind(error.error_kind),
                        structured_content: structured_content.clone(),
                        content_items: if content_items.is_empty() {
                            vec![TalosContentItem::Text {
                                text: error.message.clone(),
                            }]
                        } else {
                            content_items.clone()
                        },
                        compact_summary: Some(error.message.clone()),
                        error: Some(error),
                        pending: None,
                        policy: None,
                    }
                }
            }
            Self::Resource {
                uri,
                mime_type,
                content,
            } => ToolResultEnvelope {
                status: ToolResultStatus::Success,
                structured_content: Some(serde_json::json!({
                    "uri": uri,
                    "mime_type": mime_type,
                    "content": content,
                })),
                content_items: vec![TalosContentItem::Json {
                    value: JsonContentValue::from(serde_json::json!({
                        "uri": uri,
                        "mime_type": mime_type,
                        "content": content,
                    })),
                }],
                compact_summary: Some(build_compact_summary(content, "returned resource")),
                error: None,
                pending: None,
                policy: None,
            },
            Self::Mcp { content, is_error } => {
                if *is_error {
                    let error = ToolResultError::from_message(content);
                    ToolResultEnvelope {
                        status: status_for_error_kind(error.error_kind),
                        structured_content: None,
                        content_items: vec![TalosContentItem::Text {
                            text: error.message.clone(),
                        }],
                        compact_summary: Some(error.message.clone()),
                        error: Some(error),
                        pending: None,
                        policy: None,
                    }
                } else {
                    ToolResultEnvelope {
                        status: ToolResultStatus::Success,
                        structured_content: Some(canonical_success_content(content, None)),
                        content_items: vec![TalosContentItem::Text {
                            text: content.clone(),
                        }],
                        compact_summary: Some(build_compact_summary(content, "completed")),
                        error: None,
                        pending: None,
                        policy: None,
                    }
                }
            }
        }
    }
}

impl ToolDispatchOutcome {
    pub fn executed(payload: ToolOutputPayload) -> Self {
        Self::Executed { payload }
    }

    pub fn pending_dynamic(tool_name: impl Into<String>, arguments: Value) -> Self {
        Self::Pending {
            tool_name: tool_name.into(),
            pending: ToolPending::Dynamic { arguments },
        }
    }

    pub fn pending_question(tool_name: impl Into<String>, params: QuestionParams) -> Self {
        Self::Pending {
            tool_name: tool_name.into(),
            pending: ToolPending::Question { params },
        }
    }

    pub fn pending_sandbox_approval(
        tool_name: impl Into<String>,
        params: SandboxApprovalParams,
        request_fingerprint: String,
    ) -> Self {
        Self::Pending {
            tool_name: tool_name.into(),
            pending: ToolPending::SandboxApproval {
                params,
                request_fingerprint,
            },
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
                pending: ToolPending::Dynamic { .. },
                ..
            } => Some(ToolRenderMessage {
                kind: ToolRenderKind::Progress,
                text: format!("{tool_name}: waiting for dynamic tool result"),
            }),
            Self::Pending {
                pending: ToolPending::Question { .. },
                ..
            } => Some(ToolRenderMessage {
                kind: ToolRenderKind::Progress,
                text: format!("{tool_name}: waiting for input"),
            }),
            Self::Pending {
                pending: ToolPending::SandboxApproval { .. },
                ..
            } => Some(ToolRenderMessage {
                kind: ToolRenderKind::Progress,
                text: format!("{tool_name}: waiting for sandbox approval"),
            }),
            Self::Executed { .. } => None,
        }
    }

    pub fn render_rejected_message(&self, tool_name: &str) -> Option<ToolRenderMessage> {
        let envelope = self.envelope();
        (envelope.status == ToolResultStatus::Rejected).then(|| ToolRenderMessage {
            kind: ToolRenderKind::Rejected,
            text: format!(
                "{tool_name}: {}",
                envelope
                    .error
                    .as_ref()
                    .map(|item| item.message.as_str())
                    .unwrap_or("tool execution rejected")
            ),
        })
    }

    pub fn render_error_message(&self, tool_name: &str) -> Option<ToolRenderMessage> {
        let envelope = self.envelope();
        (envelope.status == ToolResultStatus::Failure).then(|| ToolRenderMessage {
            kind: ToolRenderKind::Error,
            text: format!(
                "{tool_name}: {}",
                envelope
                    .error
                    .as_ref()
                    .map(|item| item.message.as_str())
                    .unwrap_or("tool execution failed")
            ),
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

    pub fn envelope(&self) -> ToolResultEnvelope {
        match self {
            Self::Executed { payload } => payload.envelope(),
            Self::Pending { pending, .. } => {
                let pending = match pending {
                    ToolPending::Dynamic { arguments } => ToolResultPendingMeta {
                        pending_kind: "dynamic".to_string(),
                        prompt: None,
                        arguments: Some(arguments.clone()),
                    },
                    ToolPending::Question { params } => ToolResultPendingMeta {
                        pending_kind: "question".to_string(),
                        prompt: Some(params.question.clone()),
                        arguments: serde_json::to_value(params).ok(),
                    },
                    ToolPending::SandboxApproval { params, .. } => ToolResultPendingMeta {
                        pending_kind: "sandbox_approval".to_string(),
                        prompt: Some("sandbox approval required".to_string()),
                        arguments: serde_json::to_value(params).ok(),
                    },
                };
                ToolResultEnvelope {
                    status: ToolResultStatus::Pending,
                    structured_content: None,
                    content_items: Vec::new(),
                    compact_summary: Some("waiting for dynamic tool result".to_string()),
                    error: None,
                    pending: Some(pending),
                    policy: None,
                }
            }
        }
    }
}

fn extract_error_message(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|payload| payload.get("error"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

impl ToolResultError {
    pub fn from_message(message: impl AsRef<str>) -> Self {
        let error_kind = TalosToolErrorKind::RuntimeFailure;
        Self {
            message: message.as_ref().trim().to_string(),
            error_kind,
            retryable: is_retryable(error_kind),
            suggested_action: suggested_action_for_error(error_kind).to_string(),
            gate_source: None,
        }
    }

    pub fn next_turn_payload(&self) -> NextTurnFailurePayload {
        NextTurnFailurePayload {
            error_kind: self.error_kind,
            retryable: self.retryable,
            suggested_action: self.suggested_action.clone(),
            strategy: strategy_for_error(self.error_kind).to_string(),
            gate_source: self.gate_source.clone(),
        }
    }
}

impl ToolResultEnvelope {
    pub fn semantic_events(&self) -> Vec<&'static str> {
        match self.status {
            ToolResultStatus::Pending => vec!["tool_use_progress"],
            ToolResultStatus::Success => vec!["tool_use_summary", "tool_use_complete"],
            ToolResultStatus::Rejected => vec!["tool_use_rejected", "tool_use_complete"],
            ToolResultStatus::Failure => vec!["tool_use_error", "tool_use_complete"],
        }
    }

    pub fn next_turn_failure_payload(&self) -> Option<NextTurnFailurePayload> {
        self.error.as_ref().map(|error| error.next_turn_payload())
    }
}

fn structured_error_from_value(value: &Value) -> Option<ToolResultError> {
    let message = value
        .get("error")
        .or_else(|| value.get("message"))?
        .as_str()?
        .to_string();
    let error_kind = value
        .get("error_kind")
        .and_then(|kind| serde_json::from_value::<TalosToolErrorKind>(kind.clone()).ok())
        .unwrap_or(TalosToolErrorKind::RuntimeFailure);
    let retryable = value
        .get("retryable")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| is_retryable(error_kind));
    let suggested_action = value
        .get("suggested_action")
        .and_then(Value::as_str)
        .unwrap_or_else(|| suggested_action_for_error(error_kind))
        .to_string();
    let gate_source = value
        .get("gate_source")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(ToolResultError {
        message,
        error_kind,
        retryable,
        suggested_action,
        gate_source,
    })
}

fn is_retryable(kind: TalosToolErrorKind) -> bool {
    matches!(
        kind,
        TalosToolErrorKind::ValidationFailure
            | TalosToolErrorKind::RuntimeFailure
            | TalosToolErrorKind::StaleWriteFailure
    )
}

fn status_for_error_kind(kind: TalosToolErrorKind) -> ToolResultStatus {
    match kind {
        TalosToolErrorKind::PermissionRejection
        | TalosToolErrorKind::GovernanceRejection
        | TalosToolErrorKind::DeferredToolUnavailable => ToolResultStatus::Rejected,
        TalosToolErrorKind::ValidationFailure
        | TalosToolErrorKind::StaleWriteFailure
        | TalosToolErrorKind::RuntimeFailure => ToolResultStatus::Failure,
    }
}

fn suggested_action_for_error(kind: TalosToolErrorKind) -> &'static str {
    match kind {
        TalosToolErrorKind::ValidationFailure => "fix the arguments and retry",
        TalosToolErrorKind::PermissionRejection | TalosToolErrorKind::GovernanceRejection => {
            "choose a less privileged tool or adjust the configured tool policy"
        }
        TalosToolErrorKind::StaleWriteFailure => "re-read the target file before retrying",
        TalosToolErrorKind::DeferredToolUnavailable => {
            "use ToolSearch or start a new turn to activate the deferred tool"
        }
        TalosToolErrorKind::RuntimeFailure => "narrow scope, retry, or choose a different tool",
    }
}

fn strategy_for_error(kind: TalosToolErrorKind) -> &'static str {
    match kind {
        TalosToolErrorKind::ValidationFailure => "retry",
        TalosToolErrorKind::PermissionRejection | TalosToolErrorKind::GovernanceRejection => {
            "reject"
        }
        TalosToolErrorKind::StaleWriteFailure => "retry",
        TalosToolErrorKind::DeferredToolUnavailable => "replan",
        TalosToolErrorKind::RuntimeFailure => "retry",
    }
}

fn canonical_success_content(content: &str, structured_content: Option<&Value>) -> Value {
    structured_content
        .cloned()
        .unwrap_or_else(|| json!({ "result": content }))
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

fn build_content_items_summary(
    content_items: &[TalosContentItem],
    content: &str,
    fallback: &str,
) -> String {
    let rendered = content_items
        .iter()
        .map(TalosContentItem::display_text)
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if rendered.trim().is_empty() {
        build_compact_summary(content, fallback)
    } else {
        build_compact_summary(&rendered, fallback)
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
