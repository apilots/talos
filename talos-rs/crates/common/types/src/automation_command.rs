//! Shared Goal/Loop command semantics; clients only adapt suggestions and UI.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use ts_rs::TS;

use crate::AutomationMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationCommandDescriptor {
    pub mode: AutomationMode,
    pub command: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct AutomationCommandCatalogRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationCommandCatalogResponse {
    pub data: Vec<AutomationCommandDescriptor>,
}

pub fn automation_command_catalog() -> Vec<AutomationCommandDescriptor> {
    [
        (
            AutomationMode::Goal,
            "/goal",
            "Continue toward a verifiable objective",
        ),
        (
            AutomationMode::Loop,
            "/loop",
            "Follow up at a fixed or model-decided interval",
        ),
    ]
    .into_iter()
    .map(|(mode, command, description)| AutomationCommandDescriptor {
        mode,
        command: command.into(),
        description: description.into(),
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AutomationCommand {
    Activate {
        mode: AutomationMode,
        prompt: String,
    },
    Read {
        mode: AutomationMode,
    },
    Edit {
        mode: AutomationMode,
        #[serde(rename = "objectId")]
        object_id: Option<String>,
    },
    PauseGoal,
    ResumeGoal,
    ClearGoal,
    StopLoop {
        #[serde(rename = "loopId")]
        loop_id: String,
    },
    ClearLoop {
        #[serde(rename = "loopId")]
        loop_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationCommandResolveRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationCommandResolveResponse {
    pub command: Option<AutomationCommand>,
}

pub fn resolve_automation_command(text: &str) -> TalosResult<Option<AutomationCommand>> {
    let (name, arguments) = split_head(text.trim());
    let mode = match name {
        "/goal" => AutomationMode::Goal,
        "/loop" => AutomationMode::Loop,
        _ => return Ok(None),
    };
    let (operation, tail) = split_head(arguments);
    let command = match (mode, operation) {
        (_, "") => AutomationCommand::Read { mode },
        (AutomationMode::Loop, "list") if tail.is_empty() => AutomationCommand::Read { mode },
        (AutomationMode::Goal, "pause") if tail.is_empty() => AutomationCommand::PauseGoal,
        (AutomationMode::Goal, "resume") if tail.is_empty() => AutomationCommand::ResumeGoal,
        (AutomationMode::Goal, "clear") if tail.is_empty() => AutomationCommand::ClearGoal,
        (AutomationMode::Goal, "edit") if tail.is_empty() => AutomationCommand::Edit {
            mode,
            object_id: None,
        },
        (AutomationMode::Loop, "edit" | "stop" | "clear") => {
            if tail.is_empty() || tail.chars().any(char::is_whitespace) {
                return Err(invalid_command());
            }
            match operation {
                "edit" => AutomationCommand::Edit {
                    mode,
                    object_id: Some(tail.into()),
                },
                "stop" => AutomationCommand::StopLoop {
                    loop_id: tail.into(),
                },
                _ => AutomationCommand::ClearLoop {
                    loop_id: tail.into(),
                },
            }
        }
        (_, "list" | "pause" | "resume" | "stop" | "clear" | "edit") => {
            return Err(invalid_command())
        }
        (_, _) => AutomationCommand::Activate {
            mode,
            // Activation carries the user's prompt unchanged. Only the selected
            // Skill/model interprets cadence; this grammar owns commands, not time.
            prompt: arguments.into(),
        },
    };
    Ok(Some(command))
}

fn split_head(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(head, tail)| (head, tail.trim()))
}

fn invalid_command() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Invalid Goal/Loop command arguments",
    )
}
