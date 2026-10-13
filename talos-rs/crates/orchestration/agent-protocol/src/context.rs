use std::path::PathBuf;

use talos_config::EffectiveWorkspace;
use talos_types::{
    talos::TalosSessionEnvironment, AgentTurnRef, ModelInfo, ModelSelection, UserPermissionMode,
};

/// Immutable Session binding used to acquire one in-process Agent Session.
///
/// Service derives this value from the product Session and Workspace owners.
/// It is not a persistence Record, product Session aggregate, or Provider-native
/// session identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionContext {
    pub agent_id: String,
    pub session_id: String,
    pub workspace: EffectiveWorkspace,
}

/// Immutable execution context prepared by Service for one logical Agent Turn.
///
/// The same value crosses Coordinator and Harness unchanged. Harness reuses its
/// Session portion to acquire an Agent Session and passes the complete value to
/// that Session. It is not a complete product Session, persistence Record,
/// transport DTO, reloadable global config, or Provider-native session object.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentContext {
    /// Original Store's complete causal history_cursor; Worker compares it atomically at admission.
    pub history_cursor: Option<talos_types::SessionHistoryCursor>,
    pub session: AgentSessionContext,
    pub turn_id: String,
    pub project_id: Option<String>,
    pub working_directory: Option<PathBuf>,
    pub environment: Option<TalosSessionEnvironment>,
    pub model: ModelInfo,
    pub model_selection: ModelSelection,
    pub permission_mode: UserPermissionMode,
}

impl AgentContext {
    /// Derive the single canonical Turn identity from the nested Context.
    pub fn identity(&self) -> AgentTurnRef {
        AgentTurnRef::new(&self.session.session_id, &self.turn_id)
    }
}
