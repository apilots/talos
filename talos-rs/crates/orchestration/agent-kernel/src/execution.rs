use serde::{Deserialize, Serialize};
use talos_prompt::{PromptWarningState, SessionContinuity};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventAuthor {
    User,
    Agent,
    Tool,
    System,
}

/// Typed update to the Provider-private Session projection.
///
/// Add a variant only when Kernel execution has both a persistent producer and
/// a next-invocation consumer for one stable Session semantic. Arbitrary
/// metadata and application, user, or workspace state do not belong here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SessionStateUpdate {
    PromptWarning(PromptWarningState),
    Compaction(crate::SessionCompactionState),
    Continuity(SessionContinuity),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayDynamicToolResult {
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBranch {
    DirectAnswer,
    #[default]
    ReactiveLoop,
    ModelThinking,
}
