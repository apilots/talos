use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use talos_llm_provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ToolChoice, ToolDefinition,
};
use talos_tools_sandbox::{
    CommandRisk, CommandRiskAssessment, CommandRiskCategory, FileSystemPermissionLevel,
    NetworkSandboxPolicy,
};
use thiserror::Error;

use crate::runtimes::bash::{BashCommandKind, BashCommandRisk};

const COMMAND_MAX_CHARS: usize = 4_096;
const CWD_MAX_CHARS: usize = 1_024;
const SUMMARY_MAX_CHARS: usize = 512;
const ASSESSMENT_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_RISK_TOOL_NAME: &str = "CommandRiskAssessment";

const COMMAND_RISK_PROMPT: &str = r#"Assess the security risk of one shell command.
Call the CommandRiskAssessment tool exactly once.
Pass one JSON object matching the tool schema as arguments. Never pass quoted or stringified JSON.
Do not decide permissions or approval. Use unknown when effects cannot be determined."#;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BashCommandFacts {
    pub kind: BashCommandKind,
    pub risk: CommandRisk,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectivePermissionSummary {
    pub file_system: FileSystemPermissionLevel,
    pub network: NetworkSandboxPolicy,
    pub sandbox_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandRiskAssessmentInput {
    pub command: String,
    pub cwd: String,
    pub facts: BashCommandFacts,
    pub permissions: EffectivePermissionSummary,
}

impl CommandRiskAssessmentInput {
    pub fn new(
        command: &str,
        cwd: &Path,
        facts: BashCommandFacts,
        permissions: EffectivePermissionSummary,
    ) -> Self {
        Self {
            command: take_chars(command, COMMAND_MAX_CHARS),
            cwd: take_chars(&cwd.to_string_lossy(), CWD_MAX_CHARS),
            facts,
            permissions,
        }
    }
}

/// Assesses only command risk; implementations cannot grant permissions or execute tools.
pub trait CommandRiskAssessor: Send + Sync {
    fn assess(
        &self,
        input: CommandRiskAssessmentInput,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<CommandRiskAssessment, CommandRiskAssessmentError>>
                + Send
                + '_,
        >,
    >;
}

#[derive(Clone)]
pub struct LlmCommandRiskAssessor {
    provider: Arc<dyn LlmProvider>,
    model: String,
    timeout: Duration,
}

impl LlmCommandRiskAssessor {
    pub fn new(provider: Arc<dyn LlmProvider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            timeout: ASSESSMENT_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl CommandRiskAssessor for LlmCommandRiskAssessor {
    fn assess(
        &self,
        input: CommandRiskAssessmentInput,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<CommandRiskAssessment, CommandRiskAssessmentError>>
                + Send
                + '_,
        >,
    > {
        Box::pin(async move {
            let input = serde_json::to_string(&input).map_err(|error| {
                CommandRiskAssessmentError::InvalidOutput {
                    reason: error.to_string(),
                }
            })?;
            let response = tokio::time::timeout(
                self.timeout,
                self.provider.chat(
                    ChatRequest::builder()
                        .model(self.model.clone())
                        .messages(vec![
                            ChatMessage::system(COMMAND_RISK_PROMPT),
                            ChatMessage::user(input),
                        ])
                        .tools(vec![command_risk_tool_definition()])
                        .tool_choice(ToolChoice::Auto)
                        .parallel_tool_calls(false)
                        .temperature(0.0)
                        .max_tokens(512)
                        .build(),
                ),
            )
            .await
            .map_err(|_| CommandRiskAssessmentError::Timeout)?
            .map_err(|error| CommandRiskAssessmentError::Unavailable {
                reason: error.to_string(),
            })?;
            parse_assessment(&response)
        })
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CommandRiskAssessmentError {
    #[error("command risk assessment timed out")]
    Timeout,
    #[error("command risk assessment is unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("invalid command risk assessment: {reason}")]
    InvalidOutput { reason: String },
}

pub fn classify_bash_command(command: &str) -> BashCommandFacts {
    let normalized = command.trim().to_lowercase();
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let first = tokens
        .first()
        .copied()
        .unwrap_or_default()
        .trim_matches([';', '&', '|']);

    let dangerous = first == "rm"
        || matches!(
            first,
            "sudo" | "su" | "kill" | "pkill" | "killall" | "mkfs" | "shutdown" | "reboot"
        )
        || normalized.contains("git reset --hard")
        || normalized.contains("git clean -fd")
        || normalized.contains("dd if=")
        || normalized.contains(":(){")
        || ((normalized.contains("curl ") || normalized.contains("wget "))
            && (normalized.contains("| sh") || normalized.contains("| bash")));
    if dangerous {
        let kind = if matches!(first, "kill" | "pkill" | "killall" | "shutdown" | "reboot") {
            BashCommandKind::ProcessControl
        } else {
            BashCommandKind::Destructive
        };
        return BashCommandFacts {
            kind,
            risk: CommandRisk::Dangerous,
        };
    }

    let has_shell_control = normalized
        .chars()
        .any(|character| matches!(character, ';' | '|' | '>' | '<' | '`'))
        || normalized.contains("&&")
        || normalized.contains("||")
        || normalized.contains("$(");
    let safe = !has_shell_control
        && (matches!(first, "ls" | "pwd" | "date" | "head" | "tail" | "wc" | "rg")
            || (first == "git"
                && tokens.get(1).is_some_and(|subcommand| {
                    matches!(
                        *subcommand,
                        "status" | "diff" | "log" | "show" | "rev-parse"
                    )
                })));
    if safe {
        return BashCommandFacts {
            kind: BashCommandKind::ReadOnly,
            risk: CommandRisk::Safe,
        };
    }

    BashCommandFacts {
        kind: classify_ambiguous_kind(first, &normalized),
        risk: CommandRisk::Ambiguous,
    }
}

pub fn display_bash_risk(risk: CommandRisk) -> BashCommandRisk {
    match risk {
        CommandRisk::Safe => BashCommandRisk::Low,
        CommandRisk::Ambiguous => BashCommandRisk::Medium,
        CommandRisk::Dangerous => BashCommandRisk::High,
    }
}

pub fn unknown_assessment(summary: impl Into<String>) -> CommandRiskAssessment {
    CommandRiskAssessment {
        verdict: talos_tools_sandbox::CommandRiskVerdict::Unknown,
        categories: vec![CommandRiskCategory::Unknown],
        summary: take_chars(&summary.into(), SUMMARY_MAX_CHARS),
    }
}

fn command_risk_tool_definition() -> ToolDefinition {
    ToolDefinition::new(
        COMMAND_RISK_TOOL_NAME,
        "Return one structured command risk assessment without granting permissions.",
        Some(serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["verdict", "categories", "summary"],
            "properties": {
                "verdict": { "type": "string", "enum": ["safe", "risky", "unknown"] },
                "categories": {
                    "type": "array",
                    "maxItems": 7,
                    "uniqueItems": true,
                    "items": {
                        "type": "string",
                        "enum": [
                            "destructive_file_system",
                            "credential_or_secret_access",
                            "system_configuration",
                            "privilege_or_sandbox_bypass",
                            "remote_code_execution",
                            "process_or_service_control",
                            "unknown"
                        ]
                    }
                },
                "summary": { "type": "string", "minLength": 1, "maxLength": 512 }
            }
        })),
    )
}

fn parse_assessment(
    response: &ChatResponse,
) -> Result<CommandRiskAssessment, CommandRiskAssessmentError> {
    let calls = response.tool_calls.as_deref().unwrap_or_default();
    if calls.len() != 1 || calls[0].name != COMMAND_RISK_TOOL_NAME {
        return Err(CommandRiskAssessmentError::InvalidOutput {
            reason: "response must contain exactly one named command risk tool call".to_string(),
        });
    }
    let assessment = serde_json::from_value::<CommandRiskAssessment>(calls[0].arguments.clone())
        .map_err(|error| CommandRiskAssessmentError::InvalidOutput {
            reason: error.to_string(),
        })?;
    if assessment.summary.trim().is_empty()
        || assessment.summary.chars().count() > SUMMARY_MAX_CHARS
        || assessment.categories.len() > 7
        || assessment
            .categories
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != assessment.categories.len()
    {
        return Err(CommandRiskAssessmentError::InvalidOutput {
            reason: "assessment fields exceed bounds or contain duplicates".to_string(),
        });
    }
    Ok(assessment)
}

fn classify_ambiguous_kind(first: &str, normalized: &str) -> BashCommandKind {
    if normalized.contains('>')
        || normalized.contains("tee ")
        || matches!(first, "mv" | "cp" | "touch" | "mkdir" | "rmdir")
    {
        BashCommandKind::FilesystemWrite
    } else if matches!(first, "curl" | "wget" | "ssh" | "scp" | "rsync" | "nc") {
        BashCommandKind::Network
    } else if matches!(
        first,
        "cargo" | "pnpm" | "npm" | "yarn" | "make" | "just" | "pytest" | "go" | "mvn" | "gradle"
    ) {
        BashCommandKind::BuildOrTest
    } else {
        BashCommandKind::Unknown
    }
}

fn take_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
#[path = "command_risk_tests.rs"]
mod tests;
