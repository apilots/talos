use talos_llm_provider::ChatMessage;
use talos_types::ModelUsageKind;

/// Parameters for one Agent-owned auxiliary model call.
pub struct InnerModelChatParams {
    /// Profile owner charged for the Provider usage.
    pub user_id: String,
    /// Optional originating Session used for accounting evidence.
    pub session_id: Option<String>,
    /// Agent-owned workload classification for this call.
    pub usage_kind: ModelUsageKind,
    /// Business-owned prompt messages sent to the effective Inner Model.
    pub messages: Vec<ChatMessage>,
}
