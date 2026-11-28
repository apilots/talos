//! Session summaries derived from committed Worker terminal facts.
use super::execution_conflict;
use talos_error::{TalosError, TalosResult};

pub(crate) async fn update_completed_turn_session(
    transaction: &mut dyn talos_storage::StorageTransaction,
    session_id: &str,
    metrics: talos_agent_api::AgentRunMetrics,
    occurred_at: i64,
) -> TalosResult<()> {
    update_execution_usage(transaction, session_id, metrics, occurred_at, true).await
}

pub(crate) async fn update_compacted_session(
    transaction: &mut dyn talos_storage::StorageTransaction,
    session_id: &str,
    metrics: talos_agent_api::AgentRunMetrics,
    occurred_at: i64,
) -> TalosResult<()> {
    update_execution_usage(transaction, session_id, metrics, occurred_at, false).await
}

async fn update_execution_usage(
    transaction: &mut dyn talos_storage::StorageTransaction,
    session_id: &str,
    metrics: talos_agent_api::AgentRunMetrics,
    occurred_at: i64,
    normal_turn: bool,
) -> TalosResult<()> {
    let rows = transaction
        .query(
            "SELECT message_count, latest_turn_seq, usage_json FROM sessions WHERE session_key = ?",
            &[session_id],
        )
        .await?;
    let row = rows
        .first()
        .ok_or_else(|| execution_conflict("terminal Agent Turn Session does not exist"))?;
    let message_count = row
        .get_i64("message_count")
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| TalosError::internal("Session message count is invalid"))?
        .saturating_add(if normal_turn { 2 } else { 0 });
    let latest_turn_seq = row
        .get_i64("latest_turn_seq")
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| TalosError::internal("Session latest Turn sequence is invalid"))?
        .saturating_add(u64::from(normal_turn));
    let mut usage = row
        .get_string("usage_json")
        .ok_or_else(|| TalosError::internal("Session usage is missing"))
        .and_then(|value| {
            serde_json::from_str::<talos_types::SessionUsage>(&value).map_err(|error| {
                TalosError::internal_with_source(
                    "parse Session usage; technical details were withheld.",
                    error,
                )
            })
        })?;
    usage.tokens.accumulate(&metrics.usage);
    if let Some(context) = metrics.context.filter(|_| normal_turn) {
        usage.context = Some(context);
    }
    let usage_json = serde_json::to_string(&usage).map_err(|error| {
        TalosError::internal_with_source(
            "serialize Session usage; technical details were withheld.",
            error,
        )
    })?;
    let message_count = message_count.to_string();
    let latest_turn_seq = latest_turn_seq.to_string();
    let last_activity = occurred_at
        .checked_mul(1000)
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Worker terminal timestamp exceeds the supported range.",
            )
        })?
        .to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE sessions
            SET message_count = ?, latest_turn_seq = ?, usage_json = ?, last_activity = MAX(last_activity, CAST(? AS INTEGER))
            WHERE session_key = ?
            "#,
            &[
                message_count.as_str(),
                latest_turn_seq.as_str(),
                usage_json.as_str(),
                last_activity.as_str(),
                session_id,
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "terminal Agent Turn Session changed during settlement",
        ));
    }
    Ok(())
}
