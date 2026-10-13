use agent_pb as pb;
use talos_agent_api::AgentPendingInput;
use talos_error::TalosResult;
use talos_types::{
    DynamicToolResultParams, PendingUserAction, QuestionOption, QuestionParams,
    SandboxApprovalDecision, SandboxApprovalParams, SandboxApprovalPresentation,
    SandboxApprovalReason, SandboxFileOperation, UserActionParams, UserActionState,
};

use super::invalid;

pub fn encode_pending_input(value: AgentPendingInput) -> pb::PendingInput {
    pb::PendingInput {
        input: Some(match value {
            AgentPendingInput::Message { action_id, prompt } => {
                pb::pending_input::Input::Message(pb::PendingMessage { action_id, prompt })
            }
            AgentPendingInput::UserAction { action } => {
                pb::pending_input::Input::UserAction(encode_action(action))
            }
        }),
    }
}

pub fn decode_pending_input(value: pb::PendingInput) -> TalosResult<AgentPendingInput> {
    match value.input.ok_or_else(invalid)? {
        pb::pending_input::Input::Message(value) => Ok(AgentPendingInput::Message {
            action_id: value.action_id,
            prompt: value.prompt,
        }),
        pb::pending_input::Input::UserAction(value) => Ok(AgentPendingInput::UserAction {
            action: decode_action(value)?,
        }),
    }
}

fn encode_action(value: PendingUserAction) -> pb::PendingUserAction {
    use pb::pending_user_action::Params;
    pb::PendingUserAction {
        action_id: value.action_id,
        call_id: value.call_id,
        item_id: value.item_id,
        state: encode_state(value.state),
        params: Some(match value.params {
            UserActionParams::DynamicToolResult(value) => Params::DynamicToolName(value.tool_name),
            UserActionParams::Question(value) => Params::Question(pb::Question {
                header: value.header,
                question: value.question,
                options: value
                    .options
                    .into_iter()
                    .map(|option| pb::QuestionOption {
                        id: option.id,
                        label: option.label,
                        description: option.description,
                    })
                    .collect(),
            }),
            UserActionParams::SandboxApproval(value) => {
                Params::SandboxApproval(encode_approval(value))
            }
        }),
    }
}

fn decode_action(value: pb::PendingUserAction) -> TalosResult<PendingUserAction> {
    use pb::pending_user_action::Params;
    let params = match value.params.ok_or_else(invalid)? {
        Params::DynamicToolName(tool_name) => {
            UserActionParams::DynamicToolResult(DynamicToolResultParams { tool_name })
        }
        Params::Question(value) => UserActionParams::Question(QuestionParams {
            header: value.header,
            question: value.question,
            options: value
                .options
                .into_iter()
                .map(|option| QuestionOption {
                    id: option.id,
                    label: option.label,
                    description: option.description,
                })
                .collect(),
        }),
        Params::SandboxApproval(value) => {
            UserActionParams::SandboxApproval(decode_approval(value)?)
        }
    };
    Ok(PendingUserAction {
        action_id: value.action_id,
        call_id: value.call_id,
        item_id: value.item_id,
        state: decode_state(value.state)?,
        params,
    })
}

fn encode_approval(value: SandboxApprovalParams) -> pb::SandboxApproval {
    use pb::sandbox_approval::Presentation;
    pb::SandboxApproval {
        tool_name: value.tool_name,
        reason: match value.reason {
            SandboxApprovalReason::NetworkAccess => pb::ApprovalReason::NetworkAccess as i32,
            SandboxApprovalReason::FileSystemAccessOutsideWorkspace => {
                pb::ApprovalReason::FileSystemAccessOutsideWorkspace as i32
            }
            SandboxApprovalReason::SandboxBypass => pb::ApprovalReason::SandboxBypass as i32,
            SandboxApprovalReason::RiskyCommand => pb::ApprovalReason::RiskyCommand as i32,
        },
        presentation: Some(match value.presentation {
            SandboxApprovalPresentation::Command {
                command,
                cwd,
                risk_summary,
            } => Presentation::Command(pb::ApprovalCommand {
                command,
                cwd,
                risk_summary,
            }),
            SandboxApprovalPresentation::FileSystem { operation, paths } => {
                Presentation::FileSystem(pb::ApprovalFileSystem {
                    operation: match operation {
                        SandboxFileOperation::Read => pb::FileOperation::Read as i32,
                        SandboxFileOperation::Write => pb::FileOperation::Write as i32,
                    },
                    paths,
                })
            }
            SandboxApprovalPresentation::Network { scope } => Presentation::NetworkScope(scope),
        }),
        available_decisions: value
            .available_decisions
            .into_iter()
            .map(encode_decision)
            .collect(),
    }
}

fn decode_approval(value: pb::SandboxApproval) -> TalosResult<SandboxApprovalParams> {
    use pb::sandbox_approval::Presentation;
    let reason = match pb::ApprovalReason::try_from(value.reason) {
        Ok(pb::ApprovalReason::NetworkAccess) => SandboxApprovalReason::NetworkAccess,
        Ok(pb::ApprovalReason::FileSystemAccessOutsideWorkspace) => {
            SandboxApprovalReason::FileSystemAccessOutsideWorkspace
        }
        Ok(pb::ApprovalReason::SandboxBypass) => SandboxApprovalReason::SandboxBypass,
        Ok(pb::ApprovalReason::RiskyCommand) => SandboxApprovalReason::RiskyCommand,
        _ => return Err(invalid()),
    };
    let presentation = match value.presentation.ok_or_else(invalid)? {
        Presentation::Command(value) => SandboxApprovalPresentation::Command {
            command: value.command,
            cwd: value.cwd,
            risk_summary: value.risk_summary,
        },
        Presentation::FileSystem(value) => SandboxApprovalPresentation::FileSystem {
            operation: match pb::FileOperation::try_from(value.operation) {
                Ok(pb::FileOperation::Read) => SandboxFileOperation::Read,
                Ok(pb::FileOperation::Write) => SandboxFileOperation::Write,
                _ => return Err(invalid()),
            },
            paths: value.paths,
        },
        Presentation::NetworkScope(scope) => SandboxApprovalPresentation::Network { scope },
    };
    Ok(SandboxApprovalParams {
        tool_name: value.tool_name,
        reason,
        presentation,
        available_decisions: value
            .available_decisions
            .into_iter()
            .map(decode_decision)
            .collect::<TalosResult<_>>()?,
    })
}

fn encode_state(value: UserActionState) -> i32 {
    match value {
        UserActionState::Pending => pb::UserActionState::Pending as i32,
        UserActionState::ResponseAccepted => pb::UserActionState::ResponseAccepted as i32,
        UserActionState::Resuming => pb::UserActionState::Resuming as i32,
        UserActionState::ResumeFailed => pb::UserActionState::ResumeFailed as i32,
        UserActionState::ExecutionUnknown => pb::UserActionState::ExecutionUnknown as i32,
        UserActionState::Resolved => pb::UserActionState::Resolved as i32,
        UserActionState::Cancelled => pb::UserActionState::Cancelled as i32,
    }
}

fn decode_state(value: i32) -> TalosResult<UserActionState> {
    match pb::UserActionState::try_from(value) {
        Ok(pb::UserActionState::Pending) => Ok(UserActionState::Pending),
        Ok(pb::UserActionState::ResponseAccepted) => Ok(UserActionState::ResponseAccepted),
        Ok(pb::UserActionState::Resuming) => Ok(UserActionState::Resuming),
        Ok(pb::UserActionState::ResumeFailed) => Ok(UserActionState::ResumeFailed),
        Ok(pb::UserActionState::ExecutionUnknown) => Ok(UserActionState::ExecutionUnknown),
        Ok(pb::UserActionState::Resolved) => Ok(UserActionState::Resolved),
        Ok(pb::UserActionState::Cancelled) => Ok(UserActionState::Cancelled),
        _ => Err(invalid()),
    }
}

fn encode_decision(value: SandboxApprovalDecision) -> i32 {
    match value {
        SandboxApprovalDecision::AllowOnce => pb::ApprovalDecision::AllowOnce as i32,
        SandboxApprovalDecision::Deny => pb::ApprovalDecision::Deny as i32,
        SandboxApprovalDecision::Cancel => pb::ApprovalDecision::Cancel as i32,
    }
}

fn decode_decision(value: i32) -> TalosResult<SandboxApprovalDecision> {
    match pb::ApprovalDecision::try_from(value) {
        Ok(pb::ApprovalDecision::AllowOnce) => Ok(SandboxApprovalDecision::AllowOnce),
        Ok(pb::ApprovalDecision::Deny) => Ok(SandboxApprovalDecision::Deny),
        Ok(pb::ApprovalDecision::Cancel) => Ok(SandboxApprovalDecision::Cancel),
        _ => Err(invalid()),
    }
}
