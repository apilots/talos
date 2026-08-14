use serde::{Deserialize, Serialize};

/// Deterministic command-risk fact produced before sandbox policy evaluation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandRisk {
    Safe,
    Dangerous,
    Ambiguous,
}

/// Strict model assessment consumed only after an ambiguous deterministic result.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommandRiskAssessment {
    pub verdict: CommandRiskVerdict,
    pub categories: Vec<CommandRiskCategory>,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandRiskVerdict {
    Safe,
    Risky,
    Unknown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CommandRiskCategory {
    DestructiveFileSystem,
    CredentialOrSecretAccess,
    SystemConfiguration,
    PrivilegeOrSandboxBypass,
    RemoteCodeExecution,
    ProcessOrServiceControl,
    Unknown,
}
