use talos_agent_api::{ActiveTurnState, AgentEvent, AgentTurnRef};
use talos_error::TalosResult;

use super::record::{
    append_events, load_active_turn, load_continuation_binding, load_event_range,
    load_event_sequence,
};
use super::{
    execution_conflict, validate_user_admission, AgentActiveTurnRecord,
    AgentContinuationBindingRecord, AgentContinuationBindingStatus, AgentEventCommit,
    AgentEventRecord, ClaimContinuationParams, IdempotentCommand,
};
use crate::session::DatabaseSessionStore;

pub(super) async fn claim(
    store: &DatabaseSessionStore,
    params: ClaimContinuationParams,
) -> TalosResult<AgentEventCommit> {
    let ClaimContinuationParams {
        events,
        action_id,
        claim_id,
        expected_event_seq,
        command,
    } = params;
    let (identity, message_id) = validate_user_admission(&events)?;
    validate_nonempty("action_id", &action_id)?;
    validate_nonempty("claim_id", &claim_id)?;
    validate_command(command.as_ref(), &identity, &message_id)?;
    let mut transaction = store.storage.begin_transaction().await?;
    let active = load_active_turn(transaction.as_mut(), &identity.session_id)
        .await?
        .ok_or_else(|| execution_conflict("active Agent Turn does not exist"))?;
    let binding = load_continuation_binding(transaction.as_mut(), &identity)
        .await?
        .ok_or_else(|| execution_conflict("Agent continuation binding does not exist"))?;

    if let AgentContinuationBindingStatus::Claimed {
        claim_id: existing_claim_id,
    } = &binding.status
    {
        if existing_claim_id != &claim_id
            || binding.action_id != action_id
            || active.state != ActiveTurnState::Running
        {
            return Err(execution_conflict(
                "Agent continuation is owned by a different claim",
            ));
        }
        let records = load_existing_claim_events(transaction.as_mut(), &identity, &events).await?;
        let admission_tail = records
            .last()
            .expect("validated existing continuation admission is non-empty")
            .event_seq;
        if active.last_event_seq != admission_tail {
            return Err(execution_conflict(
                "active Agent Turn no longer matches its committed continuation admission",
            ));
        }
        transaction.rollback().await?;
        return Ok(AgentEventCommit {
            events: records,
            active_turn: active,
        });
    }

    if active.identity != identity
        || active.state != ActiveTurnState::WaitingForInput
        || active.last_event_seq != expected_event_seq
        || binding.action_id != action_id
        || binding.expected_event_seq != expected_event_seq
    {
        return Err(execution_conflict(
            "Agent continuation action or durable tail does not match",
        ));
    }
    let last_sequence = load_event_sequence(transaction.as_mut(), expected_event_seq).await?;
    if events[0].sequence != last_sequence.saturating_add(1) {
        return Err(execution_conflict(
            "Agent continuation admission sequence is not contiguous",
        ));
    }
    let records = append_events(transaction.as_mut(), events).await?;
    let last_event_seq = records
        .last()
        .expect("validated continuation admission is non-empty")
        .event_seq;
    let updated_at = chrono::Utc::now().timestamp();
    update_active_running(
        transaction.as_mut(),
        &identity,
        last_event_seq,
        expected_event_seq,
        updated_at,
    )
    .await?;
    claim_binding(transaction.as_mut(), &binding, &claim_id).await?;
    if let Some(command) = command.as_ref() {
        super::idempotency::complete_command(
            transaction.as_mut(),
            command,
            IdempotentCommand::TurnInput,
            last_event_seq,
            chrono::Utc::now().timestamp_millis(),
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(AgentEventCommit {
        events: records,
        active_turn: AgentActiveTurnRecord {
            state: ActiveTurnState::Running,
            last_event_seq,
            current_action_id: None,
            updated_at,
            ..active
        },
    })
}

fn validate_command(
    command: Option<&super::CommandLease>,
    identity: &AgentTurnRef,
    message_id: &str,
) -> TalosResult<()> {
    if command.is_some_and(|command| {
        command.command != IdempotentCommand::TurnInput
            || command.session_id != identity.session_id
            || command.turn_id != identity.turn_id
            || command.message_id != message_id
    }) {
        return Err(execution_conflict(
            "continuation input does not match its idempotency lease",
        ));
    }
    Ok(())
}

fn validate_nonempty(name: &str, value: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        return Err(execution_conflict(format!("{name} cannot be empty")));
    }
    Ok(())
}

async fn update_active_running(
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
            SET state = 'running', last_event_seq = ?, updated_at = ?
            WHERE session_id = ? AND turn_id = ?
              AND state = 'waiting_for_input' AND last_event_seq = ?
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
            "active Agent Turn changed during continuation claim",
        ));
    }
    Ok(())
}

async fn claim_binding(
    transaction: &mut dyn talos_storage::StorageTransaction,
    binding: &AgentContinuationBindingRecord,
    claim_id: &str,
) -> TalosResult<()> {
    let previous_event_seq = binding.expected_event_seq.to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_continuation_bindings
            SET status = 'claimed', claim_id = ?
            WHERE session_id = ? AND turn_id = ? AND action_id = ?
              AND status = 'waiting' AND expected_event_seq = ?
            "#,
            &[
                claim_id,
                binding.identity.session_id.as_str(),
                binding.identity.turn_id.as_str(),
                binding.action_id.as_str(),
                previous_event_seq.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "Agent continuation binding changed during claim",
        ));
    }
    Ok(())
}

async fn load_existing_claim_events(
    transaction: &mut dyn talos_storage::StorageTransaction,
    identity: &AgentTurnRef,
    expected: &[AgentEvent],
) -> TalosResult<Vec<AgentEventRecord>> {
    let first_sequence = expected
        .first()
        .expect("validated continuation admission is non-empty")
        .sequence;
    let last_sequence = expected
        .last()
        .expect("validated continuation admission is non-empty")
        .sequence;
    let records = load_event_range(transaction, identity, first_sequence, last_sequence).await?;
    let matches = records.len() == expected.len()
        && records.iter().zip(expected).all(|(record, event)| {
            record.identity == event.identity
                && record.sequence == event.sequence
                && record.payload == event.payload
        });
    if !matches {
        return Err(execution_conflict(
            "existing Agent continuation claim admission does not match",
        ));
    }
    Ok(records)
}
