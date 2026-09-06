use talos_agent_api::{
    ActiveTurnState, AgentContinuationRef, AgentEvent, AgentEventPayload, AgentTurnRef,
};
use talos_error::{TalosError, TalosResult};

use super::record::{
    append_events, load_active_turn, load_continuation_binding, load_event_sequence,
};
use super::{
    execution_conflict, AgentActiveTurnRecord, AgentContinuationBindingStatus, AgentEventCommit,
    AgentEventRecord, CommitTerminalSettlementParams,
};
use crate::session::DatabaseSessionStore;

pub(super) async fn commit_waiting(
    store: &DatabaseSessionStore,
    event: AgentEvent,
    continuation: AgentContinuationRef,
    expected_event_seq: i64,
) -> TalosResult<AgentEventCommit> {
    let action_id = waiting_action_id(&event)?.to_string();
    validate_nonempty("Provider continuation opaque_id", &continuation.opaque_id)?;
    let identity = event.identity.clone();
    let mut transaction = store.storage.begin_transaction().await?;
    let active = load_active_turn(transaction.as_mut(), &identity.session_id)
        .await?
        .ok_or_else(|| execution_conflict("active Agent Turn does not exist"))?;
    validate_active_tail(
        &active,
        &identity,
        ActiveTurnState::Running,
        expected_event_seq,
        "waiting settlement",
    )?;
    validate_next_sequence(transaction.as_mut(), &event, expected_event_seq).await?;
    let previous_binding = load_continuation_binding(transaction.as_mut(), &identity).await?;
    if let Some(binding) = &previous_binding {
        if !matches!(
            binding.status,
            AgentContinuationBindingStatus::Claimed { .. }
        ) {
            return Err(execution_conflict(
                "previous Agent continuation claim does not match waiting settlement",
            ));
        }
    }

    let mut records = append_events(transaction.as_mut(), vec![event]).await?;
    let record = records
        .pop()
        .expect("one waiting settlement produces one record");
    let updated_at = chrono::Utc::now().timestamp();
    update_active_waiting(
        transaction.as_mut(),
        &identity,
        record.event_seq,
        expected_event_seq,
        updated_at,
    )
    .await?;
    save_waiting_binding(
        transaction.as_mut(),
        &action_id,
        &continuation,
        &record,
        previous_binding.as_ref(),
    )
    .await?;
    transaction.commit().await?;
    Ok(AgentEventCommit {
        events: vec![record.clone()],
        active_turn: AgentActiveTurnRecord {
            state: ActiveTurnState::WaitingForInput,
            last_event_seq: record.event_seq,
            current_action_id: Some(action_id),
            updated_at,
            ..active
        },
    })
}

pub(super) async fn commit_terminal(
    store: &DatabaseSessionStore,
    params: CommitTerminalSettlementParams,
) -> TalosResult<AgentEventRecord> {
    let CommitTerminalSettlementParams {
        event,
        expected_event_seq,
        completed_turn_metrics,
    } = params;
    if !matches!(
        event.payload,
        AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_)
    ) {
        return Err(execution_conflict(
            "terminal settlement requires one terminal Agent event",
        ));
    }
    let identity = event.identity.clone();
    let mut transaction = store.storage.begin_transaction().await?;
    let active = load_active_turn(transaction.as_mut(), &identity.session_id)
        .await?
        .ok_or_else(|| execution_conflict("active Agent Turn does not exist"))?;
    if active.identity != identity || active.last_event_seq != expected_event_seq {
        return Err(execution_conflict(
            "active Agent Turn changed before terminal settlement",
        ));
    }
    validate_next_sequence(transaction.as_mut(), &event, expected_event_seq).await?;
    let mut records = append_events(transaction.as_mut(), vec![event]).await?;
    let record = records
        .pop()
        .expect("one terminal settlement produces one record");
    if let Some(metrics) = completed_turn_metrics {
        if !matches!(record.payload, AgentEventPayload::TurnCompleted(_)) {
            return Err(execution_conflict(
                "completed Turn metrics require a TurnCompleted event",
            ));
        }
        update_completed_turn_session(transaction.as_mut(), &identity.session_id, metrics).await?;
    }
    transaction
        .execute(
            "DELETE FROM agent_continuation_bindings WHERE session_id = ? AND turn_id = ?",
            &[identity.session_id.as_str(), identity.turn_id.as_str()],
        )
        .await?;
    let expected_param = expected_event_seq.to_string();
    let affected = transaction
        .execute(
            r#"
            DELETE FROM agent_active_turns
            WHERE session_id = ? AND turn_id = ? AND last_event_seq = ?
            "#,
            &[
                identity.session_id.as_str(),
                identity.turn_id.as_str(),
                expected_param.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "active Agent Turn changed during terminal settlement",
        ));
    }
    transaction.commit().await?;
    Ok(record)
}

async fn update_completed_turn_session(
    transaction: &mut dyn talos_storage::StorageTransaction,
    session_id: &str,
    metrics: talos_agent_api::AgentRunMetrics,
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
        .saturating_add(2);
    let latest_turn_seq = row
        .get_i64("latest_turn_seq")
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| TalosError::internal("Session latest Turn sequence is invalid"))?
        .saturating_add(1);
    let mut usage = row
        .get_string("usage_json")
        .ok_or_else(|| TalosError::internal("Session usage is missing"))
        .and_then(|value| {
            serde_json::from_str::<talos_types::SessionUsage>(&value)
                .map_err(|error| TalosError::internal(format!("parse Session usage: {error}")))
        })?;
    usage.tokens.accumulate(&metrics.usage);
    if let Some(context) = metrics.context {
        usage.context = Some(context);
    }
    let usage_json = serde_json::to_string(&usage)
        .map_err(|error| TalosError::internal(format!("serialize Session usage: {error}")))?;
    let message_count = message_count.to_string();
    let latest_turn_seq = latest_turn_seq.to_string();
    let last_activity = chrono::Utc::now().timestamp_millis().to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE sessions
            SET message_count = ?, latest_turn_seq = ?, usage_json = ?, last_activity = ?
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

fn waiting_action_id(event: &AgentEvent) -> TalosResult<&str> {
    let AgentEventPayload::WaitingForInput(pending) = &event.payload else {
        return Err(execution_conflict(
            "waiting settlement requires one WaitingForInput event",
        ));
    };
    validate_nonempty("waiting action_id", pending.action_id())?;
    Ok(pending.action_id())
}

fn validate_nonempty(name: &str, value: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        return Err(execution_conflict(format!("{name} cannot be empty")));
    }
    Ok(())
}

fn validate_active_tail(
    active: &AgentActiveTurnRecord,
    identity: &AgentTurnRef,
    state: ActiveTurnState,
    expected_event_seq: i64,
    operation: &str,
) -> TalosResult<()> {
    if active.identity != *identity
        || active.state != state
        || active.last_event_seq != expected_event_seq
    {
        return Err(execution_conflict(format!(
            "active Agent Turn changed before {operation}"
        )));
    }
    Ok(())
}

async fn validate_next_sequence(
    transaction: &mut dyn talos_storage::StorageTransaction,
    event: &AgentEvent,
    expected_event_seq: i64,
) -> TalosResult<()> {
    let last_sequence = load_event_sequence(transaction, expected_event_seq).await?;
    if event.sequence != last_sequence.saturating_add(1) {
        return Err(execution_conflict(
            "Agent settlement event sequence is not contiguous",
        ));
    }
    Ok(())
}

async fn update_active_waiting(
    transaction: &mut dyn talos_storage::StorageTransaction,
    identity: &AgentTurnRef,
    last_event_seq: i64,
    expected_event_seq: i64,
    updated_at: i64,
) -> TalosResult<()> {
    let last_event_seq = last_event_seq.to_string();
    let expected_event_seq = expected_event_seq.to_string();
    let updated_at = updated_at.to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_active_turns
            SET state = 'waiting_for_input', last_event_seq = ?, updated_at = ?
            WHERE session_id = ? AND turn_id = ?
              AND state = 'running' AND last_event_seq = ?
            "#,
            &[
                last_event_seq.as_str(),
                updated_at.as_str(),
                identity.session_id.as_str(),
                identity.turn_id.as_str(),
                expected_event_seq.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "active Agent Turn changed during waiting settlement",
        ));
    }
    Ok(())
}

async fn save_waiting_binding(
    transaction: &mut dyn talos_storage::StorageTransaction,
    action_id: &str,
    continuation: &AgentContinuationRef,
    record: &AgentEventRecord,
    previous: Option<&super::AgentContinuationBindingRecord>,
) -> TalosResult<()> {
    let provider_ref = serde_json::to_string(continuation).map_err(|error| {
        TalosError::internal(format!("serialize Agent continuation reference: {error}"))
    })?;
    let expected_event_seq = record.event_seq.to_string();
    match previous {
        None => {
            transaction
                .execute(
                    r#"
                    INSERT INTO agent_continuation_bindings (
                        session_id, turn_id, action_id, provider_ref_json,
                        expected_event_seq, status, claim_id
                    ) VALUES (?, ?, ?, ?, ?, 'waiting', NULL)
                    "#,
                    &[
                        record.identity.session_id.as_str(),
                        record.identity.turn_id.as_str(),
                        action_id,
                        provider_ref.as_str(),
                        expected_event_seq.as_str(),
                    ],
                )
                .await?;
        }
        Some(previous) => {
            let AgentContinuationBindingStatus::Claimed { claim_id } = &previous.status else {
                return Err(execution_conflict(
                    "previous Agent continuation is not claimed",
                ));
            };
            let previous_event_seq = previous.expected_event_seq.to_string();
            let affected = transaction
                .execute(
                    r#"
                    UPDATE agent_continuation_bindings
                    SET action_id = ?, provider_ref_json = ?, expected_event_seq = ?,
                        status = 'waiting', claim_id = NULL
                    WHERE session_id = ? AND turn_id = ?
                      AND action_id = ? AND expected_event_seq = ?
                      AND status = 'claimed' AND claim_id = ?
                    "#,
                    &[
                        action_id,
                        provider_ref.as_str(),
                        expected_event_seq.as_str(),
                        record.identity.session_id.as_str(),
                        record.identity.turn_id.as_str(),
                        previous.action_id.as_str(),
                        previous_event_seq.as_str(),
                        claim_id.as_str(),
                    ],
                )
                .await?;
            if affected != 1 {
                return Err(execution_conflict(
                    "Agent continuation binding changed during waiting settlement",
                ));
            }
        }
    }
    Ok(())
}
