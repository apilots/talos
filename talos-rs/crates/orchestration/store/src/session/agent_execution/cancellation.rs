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
        || !matches!(
            active.state,
            ActiveTurnState::Running | ActiveTurnState::WaitingForInput
        )
    {
        return Err(execution_conflict(
            "active Agent Turn changed before cancellation admission",
        ));
    }

    let previous_state = active.state;
    let updated_at = chrono::Utc::now().timestamp();
    update_state(
        transaction.as_mut(),
        &active,
        previous_state,
        ActiveTurnState::Cancelling,
        updated_at,
    )
    .await?;
    transaction.commit().await?;
    Ok(AgentCancellationAdmission {
        active_turn: AgentActiveTurnRecord {
            state: ActiveTurnState::Cancelling,
            updated_at,
            ..active
        },
        previous_state,
    })
}

pub(super) async fn restore(
    store: &DatabaseSessionStore,
    admission: AgentCancellationAdmission,
) -> TalosResult<AgentActiveTurnRecord> {
    if !matches!(
        admission.previous_state,
        ActiveTurnState::Running | ActiveTurnState::WaitingForInput
    ) || admission.active_turn.state != ActiveTurnState::Cancelling
    {
        return Err(execution_conflict(
            "cancellation restore requires a valid cancellation admission",
        ));
    }
    let mut transaction = store
        .begin_session_transaction(&admission.active_turn.identity.session_id)
        .await?;
    let current = load_active_turn(
        transaction.as_mut(),
        &admission.active_turn.identity.session_id,
    )
    .await?
    .ok_or_else(|| execution_conflict("active Agent Turn does not exist"))?;
    if current.identity != admission.active_turn.identity
        || current.state != ActiveTurnState::Cancelling
    {
        return Err(execution_conflict(
            "active Agent Turn changed before cancellation restore",
        ));
    }

    let updated_at = chrono::Utc::now().timestamp();
    update_state(
        transaction.as_mut(),
        &current,
        ActiveTurnState::Cancelling,
        admission.previous_state,
        updated_at,
    )
    .await?;
    transaction.commit().await?;
    let current_action_id = admission.active_turn.current_action_id;
    Ok(AgentActiveTurnRecord {
        state: admission.previous_state,
        current_action_id,
        updated_at,
        ..current
    })
}

async fn update_state(
    transaction: &mut dyn talos_storage::StorageTransaction,
    active: &AgentActiveTurnRecord,
    expected_state: ActiveTurnState,
    next_state: ActiveTurnState,
    updated_at: i64,
) -> TalosResult<()> {
    let expected_event_seq = active.last_event_seq.to_string();
    let updated_at = updated_at.to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_active_turns
            SET state = ?, updated_at = ?
            WHERE session_id = ? AND turn_id = ?
              AND state = ? AND last_event_seq = ?
            "#,
            &[
                state_name(next_state),
                updated_at.as_str(),
                active.identity.session_id.as_str(),
                active.identity.turn_id.as_str(),
                state_name(expected_state),
                expected_event_seq.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "active Agent Turn changed during cancellation state transition",
        ));
    }
    Ok(())
}

fn state_name(state: ActiveTurnState) -> &'static str {
    match state {
        ActiveTurnState::Running => "running",
        ActiveTurnState::WaitingForInput => "waiting_for_input",
        ActiveTurnState::Cancelling => "cancelling",
    }
}
