use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Row, StorageTransaction};
use talos_types::TurnCommandStatus;

use super::{
    execution_conflict, CommandLease, CommandReservation, IdempotencyRecord,
    IdempotencyReservation, IdempotentCommand,
};
use crate::session::DatabaseSessionStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredStatus {
    Reserved,
    Completed,
}

struct StoredRecord {
    record: IdempotencyRecord,
    status: StoredStatus,
    lease_id: Option<String>,
}

pub(super) async fn reserve(
    store: &DatabaseSessionStore,
    requested: CommandReservation,
) -> TalosResult<IdempotencyReservation> {
    validate_reservation(&requested)?;
    let mut transaction = store
        .begin_session_transaction(&requested.session_id)
        .await?;
    let existing = load(
        transaction.as_mut(),
        &requested.session_id,
        requested.command,
        &requested.idempotency_key,
    )
    .await?;
    let Some(existing) = existing else {
        insert_reservation(transaction.as_mut(), &requested).await?;
        transaction.commit().await?;
        return Ok(IdempotencyReservation::Reserved(lease_from_request(
            requested,
        )));
    };

    if existing
        .record
        .record_expires_at
        .is_some_and(|expires_at| expires_at <= requested.now)
    {
        transaction
            .execute(
                r#"
                DELETE FROM agent_command_idempotency
                WHERE session_id = ? AND command = ? AND idempotency_key = ?
                "#,
                &[
                    requested.session_id.as_str(),
                    requested.command.as_str(),
                    requested.idempotency_key.as_str(),
                ],
            )
            .await?;
        insert_reservation(transaction.as_mut(), &requested).await?;
        transaction.commit().await?;
        return Ok(IdempotencyReservation::Reserved(lease_from_request(
            requested,
        )));
    }

    if existing.record.request_hash != requested.request_hash {
        return Err(TalosError::new(
            ErrorCode::IdempotencyConflict,
            "Idempotency-Key was already used with a different request",
        ));
    }
    if existing.status == StoredStatus::Completed {
        transaction.rollback().await?;
        return Ok(IdempotencyReservation::Completed(existing.record));
    }
    if existing
        .record
        .lease_expires_at
        .is_some_and(|expires_at| expires_at > requested.now)
    {
        transaction.rollback().await?;
        return Ok(IdempotencyReservation::InProgress);
    }

    if existing.record.command == IdempotentCommand::TurnSubmission {
        if let Some(committed_event_seq) = committed_admission(
            transaction.as_mut(),
            &existing.record.session_id,
            &existing.record.turn_id,
        )
        .await?
        {
            let repaired = repair_completed(
                transaction.as_mut(),
                existing.record,
                committed_event_seq,
                requested.now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(IdempotencyReservation::Completed(repaired));
        }
    }

    let lease = reclaim(
        transaction.as_mut(),
        existing,
        &requested.lease_id,
        requested.lease_expires_at,
        requested.now,
    )
    .await?;
    transaction.commit().await?;
    Ok(IdempotencyReservation::Reserved(lease))
}

pub(super) async fn release(store: &DatabaseSessionStore, lease: CommandLease) -> TalosResult<()> {
    validate_lease(&lease)?;
    let mut transaction = store.begin_session_transaction(&lease.session_id).await?;
    let query = match lease.command {
        IdempotentCommand::TurnSubmission => {
            r#"
            DELETE FROM agent_command_idempotency
            WHERE session_id = ? AND command = ? AND idempotency_key = ?
              AND status = 'reserved' AND lease_id = ?
              AND NOT EXISTS (
                  SELECT 1 FROM agent_events
                  WHERE session_id = ? AND turn_id = ? AND sequence = 1
              )
            "#
        }
        IdempotentCommand::TurnInput => {
            r#"
            DELETE FROM agent_command_idempotency
            WHERE session_id = ? AND command = ? AND idempotency_key = ?
              AND status = 'reserved' AND lease_id = ?
            "#
        }
    };
    let params = match lease.command {
        IdempotentCommand::TurnSubmission => vec![
            lease.session_id.as_str(),
            lease.command.as_str(),
            lease.idempotency_key.as_str(),
            lease.lease_id.as_str(),
            lease.session_id.as_str(),
            lease.turn_id.as_str(),
        ],
        IdempotentCommand::TurnInput => vec![
            lease.session_id.as_str(),
            lease.command.as_str(),
            lease.idempotency_key.as_str(),
            lease.lease_id.as_str(),
        ],
    };
    transaction.execute(query, &params).await?;
    transaction.commit().await?;
    Ok(())
}

pub(super) async fn complete_command(
    transaction: &mut dyn StorageTransaction,
    lease: &CommandLease,
    expected_command: IdempotentCommand,
    committed_event_seq: i64,
    updated_at: i64,
) -> TalosResult<()> {
    validate_lease(lease)?;
    if lease.command != expected_command {
        return Err(execution_conflict(
            "idempotency lease does not match the committed command",
        ));
    }
    let committed_event_seq = committed_event_seq.to_string();
    let updated_at = updated_at.to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_command_idempotency
            SET status = 'completed', outcome = 'accepted', committed_event_seq = ?,
                lease_id = NULL, lease_expires_at = NULL, updated_at = ?
            WHERE session_id = ? AND command = ? AND idempotency_key = ?
              AND status = 'reserved' AND lease_id = ?
              AND request_id = ? AND turn_id = ? AND message_id = ?
            "#,
            &[
                committed_event_seq.as_str(),
                updated_at.as_str(),
                lease.session_id.as_str(),
                lease.command.as_str(),
                lease.idempotency_key.as_str(),
                lease.lease_id.as_str(),
                lease.request_id.as_str(),
                lease.turn_id.as_str(),
                lease.message_id.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "idempotency lease changed before Turn admission commit",
        ));
    }
    Ok(())
}

async fn load(
    transaction: &mut dyn StorageTransaction,
    session_id: &str,
    command: IdempotentCommand,
    idempotency_key: &str,
) -> TalosResult<Option<StoredRecord>> {
    let rows = transaction
        .query(
            r#"
            SELECT session_id, command, idempotency_key, request_hash, request_id,
                   turn_id, message_id, status, outcome, committed_event_seq,
                   lease_id, lease_expires_at, created_at, updated_at, record_expires_at
            FROM agent_command_idempotency
            WHERE session_id = ? AND command = ? AND idempotency_key = ?
            "#,
            &[session_id, command.as_str(), idempotency_key],
        )
        .await?;
    rows.first().map(parse_record).transpose()
}

async fn insert_reservation(
    transaction: &mut dyn StorageTransaction,
    requested: &CommandReservation,
) -> TalosResult<()> {
    let now = requested.now.to_string();
    let lease_expires_at = requested.lease_expires_at.to_string();
    let record_expires_at = requested.record_expires_at.unwrap_or_default().to_string();
    transaction
        .execute(
            r#"
            INSERT INTO agent_command_idempotency (
                session_id, command, idempotency_key, request_hash, request_id,
                turn_id, message_id, status, outcome, committed_event_seq,
                lease_id, lease_expires_at, created_at, updated_at, record_expires_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 'reserved', NULL, NULL, ?, ?, ?, ?, NULLIF(?, '0'))
            "#,
            &[
                requested.session_id.as_str(),
                requested.command.as_str(),
                requested.idempotency_key.as_str(),
                requested.request_hash.as_str(),
                requested.request_id.as_str(),
                requested.turn_id.as_str(),
                requested.message_id.as_str(),
                requested.lease_id.as_str(),
                lease_expires_at.as_str(),
                now.as_str(),
                now.as_str(),
                record_expires_at.as_str(),
            ],
        )
        .await?;
    Ok(())
}

async fn committed_admission(
    transaction: &mut dyn StorageTransaction,
    session_id: &str,
    turn_id: &str,
) -> TalosResult<Option<i64>> {
    let rows = transaction
        .query(
            r#"
            SELECT event_seq FROM agent_events
            WHERE session_id = ? AND turn_id = ? AND sequence = 1
            "#,
            &[session_id, turn_id],
        )
        .await?;
    Ok(rows.first().and_then(|row| row.get_i64("event_seq")))
}

async fn repair_completed(
    transaction: &mut dyn StorageTransaction,
    mut record: IdempotencyRecord,
    committed_event_seq: i64,
    updated_at: i64,
) -> TalosResult<IdempotencyRecord> {
    let committed_event_seq_param = committed_event_seq.to_string();
    let updated_at_param = updated_at.to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_command_idempotency
            SET status = 'completed', outcome = 'accepted', committed_event_seq = ?,
                lease_id = NULL, lease_expires_at = NULL, updated_at = ?
            WHERE session_id = ? AND command = ? AND idempotency_key = ?
              AND status = 'reserved'
            "#,
            &[
                committed_event_seq_param.as_str(),
                updated_at_param.as_str(),
                record.session_id.as_str(),
                record.command.as_str(),
                record.idempotency_key.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "expired idempotency reservation changed during fact repair",
        ));
    }
    record.outcome = Some(TurnCommandStatus::Accepted);
    record.committed_event_seq = Some(committed_event_seq);
    record.lease_expires_at = None;
    record.updated_at = updated_at;
    Ok(record)
}

async fn reclaim(
    transaction: &mut dyn StorageTransaction,
    existing: StoredRecord,
    next_lease_id: &str,
    next_lease_expires_at: i64,
    updated_at: i64,
) -> TalosResult<CommandLease> {
    let previous_lease_id = existing.lease_id.ok_or_else(|| {
        TalosError::internal("reserved idempotency record is missing its lease id")
    })?;
    let previous_lease_expires_at = existing.record.lease_expires_at.ok_or_else(|| {
        TalosError::internal("reserved idempotency record is missing its lease expiry")
    })?;
    let previous_lease_expires_at_param = previous_lease_expires_at.to_string();
    let next_lease_expires_at_param = next_lease_expires_at.to_string();
    let updated_at_param = updated_at.to_string();
    let affected = transaction
        .execute(
            r#"
            UPDATE agent_command_idempotency
            SET lease_id = ?, lease_expires_at = ?, updated_at = ?
            WHERE session_id = ? AND command = ? AND idempotency_key = ?
              AND status = 'reserved' AND lease_id = ? AND lease_expires_at = ?
            "#,
            &[
                next_lease_id,
                next_lease_expires_at_param.as_str(),
                updated_at_param.as_str(),
                existing.record.session_id.as_str(),
                existing.record.command.as_str(),
                existing.record.idempotency_key.as_str(),
                previous_lease_id.as_str(),
                previous_lease_expires_at_param.as_str(),
            ],
        )
        .await?;
    if affected != 1 {
        return Err(execution_conflict(
            "expired idempotency reservation changed during lease reclaim",
        ));
    }
    Ok(CommandLease {
        session_id: existing.record.session_id,
        command: existing.record.command,
        idempotency_key: existing.record.idempotency_key,
        request_id: existing.record.request_id,
        turn_id: existing.record.turn_id,
        message_id: existing
            .record
            .message_id
            .ok_or_else(|| TalosError::internal("Turn reservation is missing its message id"))?,
        lease_id: next_lease_id.to_string(),
    })
}

fn parse_record(row: &Row) -> TalosResult<StoredRecord> {
    let command = match required_string(row, "command")?.as_str() {
        "turn_submission" => IdempotentCommand::TurnSubmission,
        "turn_input" => IdempotentCommand::TurnInput,
        command => {
            return Err(TalosError::internal(
                "unknown idempotent command; technical details were withheld.",
            )
            .context(format!("unknown idempotent command: {command}")))
        }
    };
    let status = match required_string(row, "status")?.as_str() {
        "reserved" => StoredStatus::Reserved,
        "completed" => StoredStatus::Completed,
        status => {
            return Err(TalosError::internal(
                "unknown idempotency status; technical details were withheld.",
            )
            .context(format!("unknown idempotency status: {status}")))
        }
    };
    let outcome = row
        .get_string("outcome")
        .filter(|value| !value.trim().is_empty())
        .map(|value| match value.as_str() {
            "accepted" => Ok(TurnCommandStatus::Accepted),
            "failed" => Ok(TurnCommandStatus::Failed),
            "failed_no_first_event" => Ok(TurnCommandStatus::FailedNoFirstEvent),
            outcome => Err(TalosError::internal(
                "unknown Turn command outcome; technical details were withheld.",
            )
            .context(format!("unknown Turn command outcome: {outcome}"))),
        })
        .transpose()?;
    let lease_id = row
        .get_string("lease_id")
        .filter(|value| !value.trim().is_empty());
    let lease_expires_at = row.get_i64("lease_expires_at").filter(|value| *value > 0);
    match status {
        StoredStatus::Reserved if lease_id.is_none() || lease_expires_at.is_none() => {
            return Err(TalosError::internal(
                "reserved idempotency record has no live lease",
            ));
        }
        StoredStatus::Completed if lease_id.is_some() || lease_expires_at.is_some() => {
            return Err(TalosError::internal(
                "completed idempotency record still owns a lease",
            ));
        }
        StoredStatus::Reserved | StoredStatus::Completed => {}
    }
    Ok(StoredRecord {
        record: IdempotencyRecord {
            session_id: required_string(row, "session_id")?,
            command,
            idempotency_key: required_string(row, "idempotency_key")?,
            request_hash: required_string(row, "request_hash")?,
            request_id: required_string(row, "request_id")?,
            turn_id: required_string(row, "turn_id")?,
            message_id: row
                .get_string("message_id")
                .filter(|value| !value.trim().is_empty()),
            outcome,
            committed_event_seq: row
                .get_i64("committed_event_seq")
                .filter(|value| *value > 0),
            lease_expires_at,
            created_at: required_i64(row, "created_at")?,
            updated_at: required_i64(row, "updated_at")?,
            record_expires_at: row.get_i64("record_expires_at").filter(|value| *value > 0),
        },
        status,
        lease_id,
    })
}

fn lease_from_request(requested: CommandReservation) -> CommandLease {
    CommandLease {
        session_id: requested.session_id,
        command: requested.command,
        idempotency_key: requested.idempotency_key,
        request_id: requested.request_id,
        turn_id: requested.turn_id,
        message_id: requested.message_id,
        lease_id: requested.lease_id,
    }
}

fn validate_reservation(reservation: &CommandReservation) -> TalosResult<()> {
    validate_nonempty("session_id", &reservation.session_id)?;
    validate_nonempty("idempotency_key", &reservation.idempotency_key)?;
    validate_nonempty("request_hash", &reservation.request_hash)?;
    validate_nonempty("request_id", &reservation.request_id)?;
    validate_nonempty("turn_id", &reservation.turn_id)?;
    validate_nonempty("message_id", &reservation.message_id)?;
    validate_nonempty("lease_id", &reservation.lease_id)?;
    if reservation.lease_expires_at <= reservation.now {
        return Err(TalosError::internal(
            "idempotency lease expiry must be after reservation time",
        ));
    }
    if reservation
        .record_expires_at
        .is_some_and(|expires_at| expires_at <= reservation.lease_expires_at)
    {
        return Err(TalosError::internal(
            "idempotency record expiry must be after its lease",
        ));
    }
    Ok(())
}

fn validate_lease(lease: &CommandLease) -> TalosResult<()> {
    validate_nonempty("session_id", &lease.session_id)?;
    validate_nonempty("idempotency_key", &lease.idempotency_key)?;
    validate_nonempty("request_id", &lease.request_id)?;
    validate_nonempty("turn_id", &lease.turn_id)?;
    validate_nonempty("message_id", &lease.message_id)?;
    validate_nonempty("lease_id", &lease.lease_id)
}

fn validate_nonempty(name: &str, value: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        return Err(
            TalosError::internal("A required idempotency field must not be empty.")
                .context(format!("{name} must not be empty")),
        );
    }
    Ok(())
}

fn required_string(row: &Row, column: &str) -> TalosResult<String> {
    row.get_string(column)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            TalosError::internal("idempotency record is missing; technical details were withheld.")
                .context(format!("idempotency record is missing {column}"))
        })
}

fn required_i64(row: &Row, column: &str) -> TalosResult<i64> {
    row.get_i64(column).ok_or_else(|| {
        TalosError::internal("idempotency record is missing; technical details were withheld.")
            .context(format!("idempotency record is missing {column}"))
    })
}
