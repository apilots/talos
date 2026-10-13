use serde::{Deserialize, Serialize};

/// Completed history position for the immutable Session on its execution Worker.
/// Generation changes when history is cleared. The event sequence is an offset
/// within the completed Turn, never a global log offset or database Timeline cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionHistoryCursor {
    pub history_generation: u64,
    pub completed_turn_id: Option<String>,
    pub completed_event_sequence: u64,
}
