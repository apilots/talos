use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Product permission mode selected for a Session.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum UserPermissionMode {
    AskForApproval,
    ApproveForMe,
    #[default]
    FullAccess,
}

impl From<&talos_config::ToolUserPermissionModeConfig> for UserPermissionMode {
    fn from(value: &talos_config::ToolUserPermissionModeConfig) -> Self {
        match value {
            talos_config::ToolUserPermissionModeConfig::AskForApproval => Self::AskForApproval,
            talos_config::ToolUserPermissionModeConfig::ApproveForMe => Self::ApproveForMe,
            talos_config::ToolUserPermissionModeConfig::FullAccess => Self::FullAccess,
        }
    }
}

/// Current user action exposed across Service, protocol, SDK, and client boundaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingUserAction {
    pub action_id: String,
    pub call_id: String,
    #[serde(default)]
    #[ts(optional)]
    pub item_id: Option<String>,
    pub state: UserActionState,
    pub params: UserActionParams,
}

/// Typed parameters needed to render and resume a user action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", content = "params", rename_all = "camelCase")]
pub enum UserActionParams {
    DynamicToolResult(DynamicToolResultParams),
    Question(QuestionParams),
    SandboxApproval(SandboxApprovalParams),
}

/// Typed response accepted by the user-action service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", content = "response", rename_all = "camelCase")]
pub enum UserActionResponse {
    DynamicToolResult(DynamicToolResult),
    Question(QuestionAnswer),
    SandboxApproval(SandboxApprovalDecision),
}

/// Durable lifecycle state of a user action.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum UserActionState {
    Pending,
    ResponseAccepted,
    Resuming,
    ResumeFailed,
    ExecutionUnknown,
    Resolved,
    Cancelled,
}

/// Durable Timeline projection for a requested or finished user action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TimelineUserActionEvent {
    Requested {
        action: PendingUserAction,
    },
    Finished {
        action_id: String,
        state: UserActionState,
        #[serde(default)]
        #[ts(optional)]
        response: Option<UserActionResponse>,
    },
}

/// Parameters for an existing Dynamic Tool result request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolResultParams {
    pub tool_name: String,
}

/// Result submitted by an external Dynamic Tool consumer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DynamicToolResult {
    pub output: String,
    pub success: bool,
}

/// Model-authored semantic question shown to the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct QuestionParams {
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOption>,
}

/// One mutually exclusive answer offered by a semantic question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    pub description: String,
}

/// Selected or free-form answer to a semantic question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum QuestionAnswer {
    SelectedOption {
        #[schemars(rename = "optionId")]
        option_id: String,
    },
    FreeForm {
        text: String,
    },
}

/// Server-authored sandbox approval presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SandboxApprovalParams {
    pub tool_name: String,
    pub reason: SandboxApprovalReason,
    pub presentation: SandboxApprovalPresentation,
    pub available_decisions: Vec<SandboxApprovalDecision>,
}

/// Stable reason category for a sandbox approval.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SandboxApprovalReason {
    NetworkAccess,
    FileSystemAccessOutsideWorkspace,
    SandboxBypass,
    RiskyCommand,
}

/// Bounded user-facing facts for the requested sandbox capability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SandboxApprovalPresentation {
    Command {
        command: String,
        cwd: String,
        #[serde(default)]
        #[ts(optional)]
        #[schemars(rename = "riskSummary")]
        risk_summary: Option<String>,
    },
    FileSystem {
        operation: SandboxFileOperation,
        paths: Vec<String>,
    },
    Network {
        scope: String,
    },
}

/// Filesystem operation displayed by a sandbox approval.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SandboxFileOperation {
    Read,
    Write,
}

/// Fixed decision accepted for a sandbox approval.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SandboxApprovalDecision {
    AllowOnce,
    Deny,
    Cancel,
}
