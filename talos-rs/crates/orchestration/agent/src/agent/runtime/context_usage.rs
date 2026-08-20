use talos_agent_kernel::GeneralTurnResult;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::SessionStore;
use talos_types::ContextUsage;

pub(super) async fn persist_context_usage(
    session_store: &dyn SessionStore,
    session_id: &str,
    result: &GeneralTurnResult,
) -> TalosResult<()> {
    let window = &result.prompt_diagnostics.window_usage;
    if result.last_model_usage.total_tokens == 0 || !window.window_resolved {
        return Ok(());
    }
    let context_window_tokens = u32::try_from(window.context_window).unwrap_or(u32::MAX);
    let effective_prompt_budget_tokens =
        u32::try_from(window.effective_prompt_budget).unwrap_or(u32::MAX);
    let Some(context_usage) = ContextUsage::new(
        result.last_model_usage.total_tokens,
        context_window_tokens,
        effective_prompt_budget_tokens,
        chrono::Utc::now().timestamp(),
    ) else {
        return Ok(());
    };
    if !session_store
        .update_context_usage(session_id, &context_usage)
        .await?
    {
        return Err(TalosError::public(
            ErrorCode::ResourceNotFound,
            format!("session {session_id}"),
        ));
    }
    Ok(())
}
