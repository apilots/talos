use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::talos::{TalosContext, TokenUsage};
use talos_types::{CancelActiveTurnParams, TalosSession};

/// Coordinator-owned session lifecycle facade.
///
/// This trait adapts product session operations to the selected AgentHarness.
/// It must not expose Service store records or runtime-private identifiers.
#[async_trait]
pub trait SessionRuntime: Send + Sync {
    /// Resolve the Agent id that should handle the provided product context.
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    /// Invalidate any Coordinator-side cached state for a product session.
    async fn invalidate_cached_session(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    /// Compact Agent-maintained session context for the selected Harness.
    async fn compact_session(&self, session: TalosSession) -> TalosResult<TokenUsage>;

    /// Clear Agent-maintained session state without deleting product records.
    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;

    /// Close Agent-maintained session resources.
    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;

    /// Validate that a product session can be deleted from the runtime view.
    async fn prepare_delete_session(&self, session: TalosSession) -> TalosResult<()>;

    /// Cancel the active Agent turn associated with a product session.
    async fn cancel_active_turn(&self, params: CancelActiveTurnParams) -> TalosResult<()>;
}
