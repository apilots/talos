use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use talos_error::{TalosError, TalosResult};
use ts_rs::TS;

#[cfg(test)]
#[path = "subagent_tests.rs"]
mod tests;

const ROOT_AGENT_PATH: &str = "/root";
const MAX_AGENT_PATH_BYTES: usize = 512;
const MAX_AGENT_PATH_DEPTH: usize = 8;
const MAX_AGENT_PATH_SEGMENT_BYTES: usize = 64;

/// Canonical logical address of one node inside a Primary Session Agent tree.
///
/// This type owns path parsing and ancestry rules. Session IDs remain the
/// durable execution identity and must not be inferred from this path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, JsonSchema, TS)]
pub struct AgentPath(String);

impl AgentPath {
    /// Returns the root path shared by one Primary Session tree.
    pub fn root() -> Self {
        Self(ROOT_AGENT_PATH.to_string())
    }

    /// Returns the canonical path string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the number of child segments below `/root`.
    pub fn depth(&self) -> usize {
        if self.0 == ROOT_AGENT_PATH {
            0
        } else {
            self.0[ROOT_AGENT_PATH.len() + 1..].split('/').count()
        }
    }

    /// Returns the final path segment.
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("root")
    }

    /// Returns the canonical parent path, or `None` for `/root`.
    pub fn parent(&self) -> Option<Self> {
        if self.0 == ROOT_AGENT_PATH {
            return None;
        }
        let (parent, _) = self.0.rsplit_once('/')?;
        Some(Self(parent.to_string()))
    }

    /// Creates one direct child path from a validated task-name segment.
    pub fn join(&self, task_name: &str) -> TalosResult<Self> {
        validate_agent_path_segment(task_name)?;
        if self.depth() >= MAX_AGENT_PATH_DEPTH {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent path cannot exceed {MAX_AGENT_PATH_DEPTH} child segments"),
            ));
        }
        let value = format!("{}/{task_name}", self.0);
        if value.len() > MAX_AGENT_PATH_BYTES {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent path cannot exceed {MAX_AGENT_PATH_BYTES} bytes"),
            ));
        }
        Ok(Self(value))
    }

    /// Returns whether this path is a strict ancestor of `other`.
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        other
            .0
            .strip_prefix(&self.0)
            .is_some_and(|suffix| suffix.starts_with('/'))
    }
}

impl FromStr for AgentPath {
    type Err = TalosError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() > MAX_AGENT_PATH_BYTES {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent path cannot exceed {MAX_AGENT_PATH_BYTES} bytes"),
            ));
        }
        if value == ROOT_AGENT_PATH {
            return Ok(Self::root());
        }
        let suffix = value.strip_prefix("/root/").ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "agent path must be '/root' or start with '/root/'".to_string(),
            )
        })?;
        let segments = suffix.split('/').collect::<Vec<_>>();
        if segments.len() > MAX_AGENT_PATH_DEPTH {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent path cannot exceed {MAX_AGENT_PATH_DEPTH} child segments"),
            ));
        }
        for segment in segments {
            validate_agent_path_segment(segment)?;
        }
        Ok(Self(value.to_string()))
    }
}

impl fmt::Display for AgentPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for AgentPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AgentPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_str(&value).map_err(D::Error::custom)
    }
}

fn validate_agent_path_segment(segment: &str) -> TalosResult<()> {
    let valid_length = !segment.is_empty() && segment.len() <= MAX_AGENT_PATH_SEGMENT_BYTES;
    let valid_characters = segment.bytes().all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == b'_'
    });
    if !valid_length || !valid_characters || matches!(segment, "root" | "." | "..") {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid agent path segment '{segment}'"),
        ));
    }
    Ok(())
}

fn deserialize_agent_path_segment<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    validate_agent_path_segment(&value).map_err(D::Error::custom)?;
    Ok(value)
}

/// Validated caller-facing target inside an authorized Agent subtree.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AgentTarget {
    /// Selects one direct child by its task-name segment.
    DirectChild {
        #[schemars(length(min = 1, max = 64), regex(pattern = "^[a-z0-9_]+$"))]
        name: String,
    },
    /// Selects one canonical descendant in the caller's subtree.
    Descendant { path: AgentPath },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum AgentTargetInput {
    DirectChild {
        #[serde(deserialize_with = "deserialize_agent_path_segment")]
        name: String,
    },
    Descendant {
        path: AgentPath,
    },
}

impl<'de> Deserialize<'de> for AgentTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match AgentTargetInput::deserialize(deserializer)? {
            AgentTargetInput::DirectChild { name } => Ok(Self::DirectChild { name }),
            AgentTargetInput::Descendant { path } => Ok(Self::Descendant { path }),
        }
    }
}

impl AgentTarget {
    /// Creates a canonical target from an already validated Agent path.
    pub fn canonical(path: AgentPath) -> Self {
        Self::Descendant { path }
    }

    /// Returns the caller-provided normalized target string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::DirectChild { name } => name,
            Self::Descendant { path } => path.as_str(),
        }
    }

    /// Resolves this target and returns `None` when it is outside the caller subtree.
    pub fn resolve_descendant(&self, caller: &AgentPath) -> TalosResult<Option<AgentPath>> {
        match self {
            Self::DirectChild { name } => caller.join(name).map(Some),
            Self::Descendant { path } if caller.is_ancestor_of(path) => Ok(Some(path.clone())),
            Self::Descendant { .. } => Ok(None),
        }
    }
}

/// Exact Session Turn identity constructed by a trusted application adapter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnRef {
    pub session_id: String,
    pub turn_id: String,
}

/// Trusted identity of the Agent currently invoking a collaboration operation.
///
/// Tool handlers construct this object from runtime context. Model arguments
/// must never supply or override these fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCaller {
    pub session_id: String,
    pub turn_id: String,
    pub agent_id: String,
}

impl AgentCaller {
    /// Return the durable Session Turn reference used by child ownership.
    pub fn turn_ref(&self) -> AgentTurnRef {
        AgentTurnRef {
            session_id: self.session_id.clone(),
            turn_id: self.turn_id.clone(),
        }
    }
}

/// Stable handle for a child Session and its immutable owner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRef {
    pub path: AgentPath,
    pub session_id: String,
    pub parent: AgentTurnRef,
}

/// Exact durable child identity and the initial Turn accepted for execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSpawnResult {
    pub subagent: SubagentRef,
    pub agent_id: String,
    pub turn: AgentTurnRef,
}

/// Complete input required to create and start one child Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSubagentRequest {
    pub caller: AgentCaller,
    pub task_name: String,
    pub prompt: String,
}

/// Public arguments accepted by `spawn_agent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnAgentArguments {
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[a-z0-9_]+$"))]
    pub task_name: String,
    #[schemars(length(min = 1, max = 32768))]
    pub prompt: String,
}

/// Public arguments accepted by `send_message`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageArguments {
    pub target: AgentTarget,
    #[schemars(length(min = 1, max = 32768))]
    pub message: String,
}

/// Public arguments accepted by `wait_agent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitAgentArguments {
    pub target: AgentTarget,
    #[schemars(range(min = 0, max = 60000))]
    pub timeout_ms: u32,
}

/// Public arguments accepted by `interrupt_agent`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InterruptAgentArguments {
    pub target: AgentTarget,
}

/// Public arguments accepted by `followup_task`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FollowupTaskArguments {
    pub target: AgentTarget,
    #[schemars(length(min = 1, max = 32768))]
    pub prompt: String,
}

/// Public arguments accepted by `list_agents`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct ListAgentsArguments {}

/// Current state projected from the child Session's existing Turn facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SubagentOutcome {
    Running {
        #[serde(rename = "turnId")]
        turn_id: String,
    },
    WaitingUserAction {
        #[serde(rename = "turnId")]
        turn_id: String,
    },
    Cancelling {
        #[serde(rename = "turnId")]
        turn_id: String,
    },
    Completed {
        #[serde(rename = "turnId")]
        turn_id: String,
        output: String,
        truncated: bool,
    },
    Failed {
        #[serde(rename = "turnId")]
        turn_id: String,
        error: String,
        truncated: bool,
    },
    Interrupted {
        #[serde(rename = "turnId")]
        turn_id: String,
    },
}

/// Kind of bounded assistant activity exposed by a Subagent snapshot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum SubagentUpdateKind {
    Commentary,
    FinalAnswer,
}

/// Latest bounded assistant activity for the current child Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentUpdate {
    pub kind: SubagentUpdateKind,
    pub text: String,
    pub truncated: bool,
}

/// Durable child identity paired with its projected execution state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSnapshot {
    pub subagent: SubagentRef,
    pub agent_id: String,
    pub latest_update: Option<SubagentUpdate>,
    pub outcome: SubagentOutcome,
}

/// Bounded wait request constructed from trusted Primary Session context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WaitSubagentRequest {
    pub caller: AgentCaller,
    pub target: AgentTarget,
    pub timeout_ms: u64,
}

/// One recursively nested Agent tree projected from durable child Sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTreeSnapshot {
    pub children: Vec<AgentTreeNode>,
}

/// One Agent tree node and its direct child nodes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTreeNode {
    pub snapshot: SubagentSnapshot,
    pub children: Vec<AgentTreeNode>,
}

/// Exact child interruption request constructed from trusted parent context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterruptSubagentRequest {
    pub caller: AgentCaller,
    pub target: AgentTarget,
}

/// New Turn request for a terminal child Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FollowUpSubagentRequest {
    pub caller: AgentCaller,
    pub target: AgentTarget,
    pub prompt: String,
}

/// Message request constructed from trusted caller context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SendSubagentMessageRequest {
    pub caller: AgentCaller,
    pub target: AgentTarget,
    pub message: String,
}

/// Accepted message delivery to one exact running child Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResult {
    pub subagent: SubagentRef,
    pub agent_id: String,
    pub turn: AgentTurnRef,
    pub accepted: bool,
}

/// Agent tree query constructed from trusted caller context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListSubagentsRequest {
    pub caller: AgentCaller,
}

/// Canonical strongly typed result emitted by collaboration Tools.
///
/// Generic Tool transport may serialize this enum once, but business and
/// presentation consumers must decode this tagged contract rather than select
/// a result shape from the Tool name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "operation", content = "result", rename_all = "camelCase")]
pub enum CollaborationToolResult {
    SpawnAgent(SubagentSpawnResult),
    SendMessage(SendMessageResult),
    WaitAgent(SubagentSnapshot),
    InterruptAgent(SubagentSnapshot),
    FollowupTask(SubagentSnapshot),
    ListAgents(AgentTreeSnapshot),
}
