//! Product cancellation requests never rewrite observed Worker execution facts.
use talos_agent_api::ActiveTurnState;
use talos_error::TalosResult;

use super::record::load_active_turn;
use super::{execution_conflict, AgentActiveTurnRecord, AgentCancellationAdmission};
use crate::session::DatabaseSessionStore;

pub(super) async fn begin(
    store: &DatabaseSessionStore,
    identity: &talos_agent_api::AgentTurnRef,
) -> TalosResult<AgentCancellationAdmission> {
    let mut transaction = store
        .begin_session_transaction(&identity.session_id)
        .await?;
    let active = load_active_turn(transaction.as_mut(), &identity.session_id)
        .await?
        .ok_or_else(|| execution_conflict("active Agent Turn does not exist"))?;
    if active.identity != *identity
        || active.cancellation_requested
        || !matches!(
            active.state,
            ActiveTurnState::Running | ActiveTurnState::WaitingForInput
        )
    {
        return Err(execution_conflict(
            "active Agent Turn changed before cancellation request",
        ));
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    let affected = transaction.execute(
        "UPDATE agent_active_turns SET cancellation_request_id=? WHERE session_id=? AND turn_id=? AND cancellation_request_id IS NULL",
        &[&request_id, &identity.session_id, &identity.turn_id],
    ).await?;
    if affected != 1 {
        return Err(execution_conflict(
            "active Agent Turn changed during cancellation request",
        ));
    }
    transaction.commit().await?;
    Ok(AgentCancellationAdmission {
        active_turn: AgentActiveTurnRecord {
            cancellation_requested: true,
            ..active
        },
        request_id,
    })
}

pub(super) async fn restore(
    store: &DatabaseSessionStore,
    admission: AgentCancellationAdmission,
) -> TalosResult<AgentActiveTurnRecord> {
    let identity = &admission.active_turn.identity;
    let mut transaction = store
        .begin_session_transaction(&identity.session_id)
        .await?;
    let affected = transaction.execute(
        "UPDATE agent_active_turns SET cancellation_request_id=NULL WHERE session_id=? AND turn_id=? AND cancellation_request_id=?",
        &[&identity.session_id, &identity.turn_id, &admission.request_id],
    ).await?;
    if affected != 1 {
        return Err(execution_conflict(
            "active Agent Turn no longer has this cancellation request",
        ));
    }
    let current = load_active_turn(transaction.as_mut(), &identity.session_id)
        .await?
        .ok_or_else(|| {
            execution_conflict("active Agent Turn settled during cancellation request")
        })?;
    transaction.commit().await?;
    Ok(current)
}
