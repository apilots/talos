use agent_protocol::{
    AgentExecutionCommand, AgentExecutionSettlement, AgentRunCheckpoint, AgentSessionContext,
    MAX_EXECUTION_EVENT_BYTES,
};
use sqlx::{Sqlite, Transaction};
use talos_agent_api::{trajectory::SessionHistorySupplement, AgentEvent, AgentTurnRef};
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::{conflict, database_error, decode, encode, WorkerExecutionStore};

impl WorkerExecutionStore {
    /// Commit the original input and admission facts before releasing execution.
    pub async fn admit(
        &self,
        session: &AgentSessionContext,
        command: &AgentExecutionCommand,
        events: Vec<AgentEvent>,
        checkpoint: &AgentRunCheckpoint,
    ) -> TalosResult<()> {
        if self.is_retired(&session.session_id).await? {
            return Err(TalosError::new(
                ErrorCode::SessionNotFound,
                "Worker Session was permanently deleted.",
            ));
        }
        let first = events.first().ok_or_else(conflict)?;
        let identity = first.identity.clone();
        let input_id = super::command_key(command)?;
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        sqlx::query(
            "INSERT OR IGNORE INTO worker_sessions(session_id, binding_json) VALUES (?, ?)",
        )
        .bind(&session.session_id)
        .bind(encode(session)?)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        let retired: i64 =
            sqlx::query_scalar("SELECT retired FROM worker_sessions WHERE session_id=?")
                .bind(&session.session_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(database_error)?;
        if retired != 0 {
            return Err(TalosError::new(
                ErrorCode::SessionNotFound,
                "Worker Session was permanently deleted.",
            ));
        }
        if identity.session_id != session.session_id
            || active
                .as_ref()
                .is_some_and(|turn| *turn != identity.turn_id)
        {
            return Err(conflict());
        }
        let exists: Option<String> = sqlx::query_scalar("SELECT request_json FROM worker_admissions WHERE session_id = ? AND generation = ? AND input_id = ?")
            .bind(&session.session_id).bind(generation as i64).bind(&input_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        let request = encode(command)?;
        if let Some(original) = exists {
            return if original == request {
                Err(conflict())
            } else {
                Err(TalosError::new(ErrorCode::IdempotencyConflict, "Worker command identity was already admitted with different input or configuration."))
            };
        }
        if let AgentExecutionCommand::Run { params } = command {
            if let (
                Some(limit),
                talos_types::SessionTreeIdentity::Subagent {
                    tree_root_session_id,
                    ..
                },
            ) = (params.subagent_limit, &params.context.tree_identity)
            {
                let completed:i64=sqlx::query_scalar("SELECT COUNT(*) FROM worker_events WHERE session_id=? AND generation=? AND json_extract(settlement_json,'$.type')='finished'").bind(&session.session_id).bind(generation as i64).fetch_one(&mut *tx).await.map_err(database_error)?;
                if completed == 0 {
                    let active:i64=sqlx::query_scalar("SELECT COUNT(DISTINCT s.session_id) FROM worker_sessions s JOIN worker_admissions a ON a.session_id=s.session_id AND a.generation=s.generation AND a.turn_id=s.active_turn_id WHERE s.active_turn_id IS NOT NULL AND json_extract(a.request_json,'$.params.context.tree_identity.tree_root_session_id')=?").bind(tree_root_session_id).fetch_one(&mut *tx).await.map_err(database_error)?;
                    if active >= i64::from(limit.get()) {
                        return Err(TalosError::new(
                            ErrorCode::SessionBudgetExceeded,
                            "This Session has reached its active child Turn limit.",
                        ));
                    }
                }
            }
        }
        let (first_offset, last_offset) =
            append(&mut tx, generation, &events, None, Some(command)).await?;
        write_checkpoint(&mut tx, generation, &identity, checkpoint).await?;
        sqlx::query("INSERT INTO worker_admissions(session_id,generation,turn_id,input_id,request_json,first_offset,last_offset) VALUES (?,?,?,?,?,?,?)")
            .bind(&session.session_id).bind(generation as i64).bind(&identity.turn_id).bind(input_id).bind(request).bind(first_offset).bind(last_offset)
            .execute(&mut *tx).await.map_err(database_error)?;
        sqlx::query("UPDATE worker_sessions SET active_turn_id = ? WHERE session_id = ?")
            .bind(&identity.turn_id)
            .bind(&session.session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        tx.commit().await.map_err(database_error)
    }

    /// Events, reducer checkpoint and terminal ownership update share one transaction.
    pub async fn append(
        &self,
        session: &AgentSessionContext,
        events: Vec<AgentEvent>,
        checkpoint: &AgentRunCheckpoint,
        settlement: Option<AgentExecutionSettlement>,
    ) -> TalosResult<()> {
        let last = events.last().ok_or_else(conflict)?;
        let terminal = last.payload.is_terminal();
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        sqlx::query("UPDATE worker_sessions SET generation = generation WHERE session_id = ?")
            .bind(&session.session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        if active.as_deref() != Some(last.identity.turn_id.as_str()) {
            return Err(conflict());
        }
        append(&mut tx, generation, &events, settlement, None).await?;
        write_checkpoint(&mut tx, generation, &last.identity, checkpoint).await?;
        if terminal {
            sqlx::query("UPDATE worker_sessions SET active_turn_id = NULL WHERE session_id = ? AND active_turn_id = ?")
                .bind(&session.session_id).bind(&last.identity.turn_id).execute(&mut *tx).await.map_err(database_error)?;
        }
        tx.commit().await.map_err(database_error)
    }

    pub async fn advance_memory_progress(
        &self,
        session_id: &str,
        expected: u64,
        next: u64,
    ) -> TalosResult<()> {
        if next < expected {
            return Err(conflict());
        }
        let expected = i64::try_from(expected).map_err(|_| conflict())?;
        let next = i64::try_from(next).map_err(|_| conflict())?;
        let changed = sqlx::query("UPDATE worker_sessions SET memory_processed_until = ? WHERE session_id = ? AND memory_processed_until = ? AND ? <= COALESCE((SELECT MAX(event_offset) FROM worker_events WHERE session_id = ? AND generation = worker_sessions.generation),0)")
            .bind(next).bind(session_id).bind(expected).bind(next).bind(session_id).execute(&self.pool).await.map_err(database_error)?.rows_affected();
        if changed != 1 {
            return Err(conflict());
        }
        Ok(())
    }

    pub async fn append_supplement(&self, supplement: SessionHistorySupplement) -> TalosResult<()> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let session_id = supplement.identity.session_id.clone();
        sqlx::query("UPDATE worker_sessions SET generation = generation WHERE session_id = ?")
            .bind(&session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let value: Option<(i64, Option<String>)> = sqlx::query_as(
            "SELECT generation, active_turn_id FROM worker_sessions WHERE session_id = ?",
        )
        .bind(&session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database_error)?;
        let (generation, active) = value.ok_or_else(conflict)?;
        if active.as_deref() != Some(supplement.identity.turn_id.as_str()) {
            return Err(conflict());
        }
        let old: Option<String> = sqlx::query_scalar("SELECT supplement_json FROM worker_supplements WHERE session_id = ? AND generation = ? AND turn_id = ?")
            .bind(&session_id).bind(generation).bind(&supplement.identity.turn_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        let merged = match old {
            Some(value) => super::supplement_merge::append(decode(&value)?, supplement)?,
            None => super::supplement_merge::append_empty(supplement)?,
        };
        let body = encode(&merged)?;
        let mut metadata = merged.clone();
        metadata.payloads.clear();
        if body.len() > 32 * 1024 * 1024
            || encode(&metadata)?.len() > agent_protocol::MAX_EXECUTION_CAPTURE_BYTES
        {
            return Err(TalosError::new(
                ErrorCode::SessionBudgetExceeded,
                "Execution capture exceeds its bounded recording limit.",
            ));
        }
        sqlx::query("INSERT INTO worker_supplements(session_id,generation,turn_id,supplement_json) VALUES (?,?,?,?) ON CONFLICT(session_id,generation,turn_id) DO UPDATE SET supplement_json = excluded.supplement_json")
            .bind(&session_id).bind(generation).bind(&merged.identity.turn_id).bind(body).execute(&mut *tx).await.map_err(database_error)?;
        tx.commit().await.map_err(database_error)
    }

    /// Remove payloads atomically while retaining the generation fence.
    pub async fn clear(&self, session: &AgentSessionContext) -> TalosResult<()> {
        self.reset(session, false).await
    }

    /// Keep a durable retirement fence after deleting execution payloads.
    pub async fn retire(&self, session: &AgentSessionContext) -> TalosResult<()> {
        if self.is_retired(&session.session_id).await? {
            return Ok(());
        }
        self.reset(session, true).await
    }

    async fn reset(&self, session: &AgentSessionContext, retire: bool) -> TalosResult<()> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        sqlx::query("UPDATE worker_sessions SET generation = generation WHERE session_id = ?")
            .bind(&session.session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        sqlx::query("INSERT OR IGNORE INTO worker_sessions(session_id,binding_json) VALUES (?,?)")
            .bind(&session.session_id)
            .bind(encode(session)?)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        let retired: i64 =
            sqlx::query_scalar("SELECT retired FROM worker_sessions WHERE session_id=?")
                .bind(&session.session_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(database_error)?;
        if !retire && retired != 0 {
            return Err(TalosError::new(
                ErrorCode::SessionNotFound,
                "Worker Session was permanently deleted.",
            ));
        }
        if active.is_some() {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Worker Session must be idle before clearing execution history.",
            ));
        }
        // A failed product clear may be followed by deletion. Retire that already
        // cleared generation in place so another failed product commit remains
        // within the same exact next-generation retry fence.
        let generation_increment = if retire && generation > 0 {
            let has_payloads: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM worker_events WHERE session_id=?) OR EXISTS(SELECT 1 FROM worker_admissions WHERE session_id=?) OR EXISTS(SELECT 1 FROM worker_checkpoints WHERE session_id=?) OR EXISTS(SELECT 1 FROM worker_supplements WHERE session_id=?)",
            ).bind(&session.session_id).bind(&session.session_id).bind(&session.session_id)
                .bind(&session.session_id).fetch_one(&mut *tx).await.map_err(database_error)?;
            i64::from(has_payloads != 0)
        } else {
            1
        };
        for table in [
            "worker_events",
            "worker_admissions",
            "worker_checkpoints",
            "worker_supplements",
        ] {
            sqlx::query(&format!("DELETE FROM {table} WHERE session_id = ?"))
                .bind(&session.session_id)
                .execute(&mut *tx)
                .await
                .map_err(database_error)?;
        }
        sqlx::query("UPDATE worker_sessions SET generation = generation + ?, memory_processed_until = 0, retired = ? WHERE session_id = ?").bind(generation_increment).bind(i64::from(retire)).bind(&session.session_id).execute(&mut *tx).await.map_err(database_error)?;
        tx.commit().await.map_err(database_error)
    }
}

async fn append(
    tx: &mut Transaction<'_, Sqlite>,
    generation: u64,
    events: &[AgentEvent],
    settlement: Option<AgentExecutionSettlement>,
    admission: Option<&AgentExecutionCommand>,
) -> TalosResult<(i64, i64)> {
    let first = events.first().ok_or_else(conflict)?;
    let previous: Option<i64> = sqlx::query_scalar("SELECT MAX(sequence) FROM worker_events WHERE session_id = ? AND generation = ? AND turn_id = ?")
        .bind(&first.identity.session_id).bind(generation as i64).bind(&first.identity.turn_id).fetch_one(&mut **tx).await.map_err(database_error)?;
    let mut sequence = previous.unwrap_or(0) as u64;
    let mut first_offset = 0;
    let mut last_offset = 0;
    for (index, event) in events.iter().enumerate() {
        if event.identity != first.identity || event.sequence != sequence + 1 {
            return Err(conflict());
        }
        let json = encode(event)?;
        let terminal_metadata = (index + 1 == events.len())
            .then_some(settlement.as_ref())
            .flatten();
        if event.payload.is_terminal()
            && !matches!(
                terminal_metadata,
                Some(AgentExecutionSettlement::Finished { .. })
            )
        {
            return Err(conflict());
        }
        let (command, automation_input, execution_model) = admission
            .map(|command| super::command_metadata(command, event))
            .unwrap_or_default();
        let history_supplement = if event.payload.is_terminal() {
            let stored:Option<String>=sqlx::query_scalar("SELECT supplement_json FROM worker_supplements WHERE session_id=? AND generation=? AND turn_id=?").bind(&event.identity.session_id).bind(generation as i64).bind(&event.identity.turn_id).fetch_optional(&mut **tx).await.map_err(database_error)?;
            stored
                .as_deref()
                .map(decode::<SessionHistorySupplement>)
                .transpose()?
                .map(|mut capture| {
                    capture.payloads.clear();
                    capture
                })
        } else {
            None
        };
        let capture_json = history_supplement.as_ref().map(encode).transpose()?;
        let original: Option<String>=sqlx::query_scalar("SELECT request_json FROM worker_admissions WHERE session_id=? AND generation=? AND turn_id=? ORDER BY first_offset LIMIT 1").bind(&event.identity.session_id).bind(generation as i64).bind(&event.identity.turn_id).fetch_optional(&mut **tx).await.map_err(database_error)?;
        let execution_purpose = match admission {
            Some(command) => super::command_purpose(command),
            None => super::command_purpose(
                &original
                    .as_deref()
                    .map(decode::<agent_protocol::AgentExecutionCommand>)
                    .transpose()?
                    .ok_or_else(|| TalosError::internal("Worker event admission is missing."))?,
            ),
        };
        let fact = agent_protocol::AgentCommittedEvent {
            execution_purpose,
            history_supplement,
            execution_model,
            cursor: agent_protocol::AgentSessionEventCursor {
                session_id: event.identity.session_id.clone(),
                generation,
                offset: i64::MAX,
            },
            event: event.clone(),
            settlement: terminal_metadata.cloned(),
            command,
            automation_input,
        };
        if encode(&fact)?.len() > MAX_EXECUTION_EVENT_BYTES {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent event exceeds the bounded execution event limit.",
            ));
        }
        let offset: i64 = sqlx::query_scalar("INSERT INTO worker_events(session_id,generation,turn_id,sequence,event_json,settlement_json,capture_json) VALUES (?,?,?,?,?,?,?) RETURNING event_offset")
            .bind(&event.identity.session_id).bind(generation as i64).bind(&event.identity.turn_id).bind(event.sequence as i64).bind(json)
            .bind(terminal_metadata.map(encode).transpose()?).bind(capture_json).fetch_one(&mut **tx).await.map_err(database_error)?;
        if first_offset == 0 {
            first_offset = offset;
        }
        last_offset = offset;
        sequence = event.sequence;
    }
    Ok((first_offset, last_offset))
}

async fn write_checkpoint(
    tx: &mut Transaction<'_, Sqlite>,
    generation: u64,
    identity: &AgentTurnRef,
    checkpoint: &AgentRunCheckpoint,
) -> TalosResult<()> {
    // Every committed open snapshot must still fit its later complete closure event.
    // Reject an overflowing update before replacing the last safe checkpoint.
    if encode(&checkpoint.metrics)?.len()
        > MAX_EXECUTION_EVENT_BYTES - agent_protocol::MAX_EXECUTION_CAPTURE_BYTES - 16 * 1024
    {
        return Err(TalosError::new(
            ErrorCode::SessionBudgetExceeded,
            "Agent metrics exceed the bounded terminal snapshot limit.",
        ));
    }
    for message in &checkpoint.messages {
        if encode(message)?.len() > MAX_EXECUTION_EVENT_BYTES - 16 * 1024 {
            return Err(TalosError::new(
                ErrorCode::SessionBudgetExceeded,
                "Agent message exceeds the bounded execution snapshot limit.",
            ));
        }
    }
    sqlx::query("INSERT INTO worker_checkpoints(session_id,generation,turn_id,checkpoint_json) VALUES (?,?,?,?) ON CONFLICT(session_id,generation,turn_id) DO UPDATE SET checkpoint_json = excluded.checkpoint_json")
        .bind(&identity.session_id).bind(generation as i64).bind(&identity.turn_id).bind(encode(checkpoint)?).execute(&mut **tx).await.map_err(database_error)?;
    Ok(())
}
