use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::talos::{TalosContext, TalosUsage};
use talos_types::TalosSession;

#[async_trait]
pub trait SessionRuntime: Send + Sync {
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn invalidate_cached_session(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn compact_session(&self, session: TalosSession) -> TalosResult<TalosUsage>;

    async fn clear_session(&self, session: TalosSession) -> TalosResult<()>;

    async fn close_session(&self, session: TalosSession) -> TalosResult<()>;

    async fn prepare_delete_session(&self, session: TalosSession) -> TalosResult<()>;

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;
}
