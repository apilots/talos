use agent_protocol::{
    AgentCommittedEvent, AgentExecutionSettlement, AgentRunCheckpoint, AgentSessionContext,
    AgentSessionEventsParams, AgentSessionEventsResult, MAX_EXECUTION_PAGE_BYTES,
    MAX_EXECUTION_PAGE_EVENTS,
};
use talos_agent_api::{trajectory::SessionHistorySupplement, AgentEvent, AgentTurnRef};
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::{cursor, database_error, decode, encode, WorkerExecutionStore};

type StoredExecutionEvent = (i64, String, Option<String>, Option<String>, Option<String>);

impl WorkerExecutionStore {
    /// Called only after the host acquires the existing namespace file lock.
    pub async fn interrupted_executions(
        &self,
    ) -> TalosResult<Vec<(AgentSessionContext, AgentTurnRef, AgentRunCheckpoint)>> {
        let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT s.binding_json, s.active_turn_id, c.checkpoint_json FROM worker_sessions s JOIN worker_checkpoints c ON c.session_id=s.session_id AND c.generation=s.generation AND c.turn_id=s.active_turn_id WHERE s.active_turn_id IS NOT NULL AND COALESCE((SELECT json_extract(e.settlement_json, '$.type') FROM worker_events e WHERE e.session_id=s.session_id AND e.generation=s.generation AND e.turn_id=s.active_turn_id ORDER BY e.event_offset DESC LIMIT 1), '') <> 'waitingForInput' LIMIT 256")
            .fetch_all(&self.pool).await.map_err(database_error)?;
        rows.into_iter()
            .map(|(binding, turn, checkpoint)| {
                let context: AgentSessionContext = decode(&binding)?;
                let identity = AgentTurnRef::new(&context.session_id, turn);
                Ok((context, identity, decode(&checkpoint)?))
            })
            .collect()
    }

    pub async fn execution_state(
        &self,
        session: &AgentSessionContext,
    ) -> TalosResult<Option<agent_protocol::SessionExecutionState>> {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM worker_sessions WHERE session_id = ?")
                .bind(&session.session_id)
                .fetch_one(&self.pool)
                .await
                .map_err(database_error)?;
        if exists == 0 {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        let terminal: Option<String> = sqlx::query_scalar("SELECT event_json FROM worker_events WHERE session_id = ? AND generation = ? AND json_extract(settlement_json, '$.type') = 'finished' ORDER BY event_offset DESC LIMIT 1").bind(&session.session_id).bind(generation as i64).fetch_optional(&mut *tx).await.map_err(database_error)?;
        let terminal = terminal.as_deref().map(decode::<AgentEvent>).transpose()?;
        tx.commit().await.map_err(database_error)?;
        Ok(Some(agent_protocol::SessionExecutionState {
            history_cursor: talos_types::SessionHistoryCursor {
                history_generation: generation,
                completed_turn_id: terminal
                    .as_ref()
                    .map(|event| event.identity.turn_id.clone()),
                completed_event_sequence: terminal.map_or(0, |event| event.sequence),
            },
            active_turn: active
                .as_ref()
                .map(|turn| AgentTurnRef::new(&session.session_id, turn)),
            coordinator_id: None,
            handoff_ready: active.is_none(),
        }))
    }

    pub async fn read_events(
        &self,
        params: AgentSessionEventsParams,
    ) -> TalosResult<AgentSessionEventsResult> {
        let exists: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM worker_sessions WHERE session_id=?")
                .bind(&params.session.session_id)
                .fetch_one(&self.pool)
                .await
                .map_err(database_error)?;
        if exists == 0 {
            if params.after_cursor.as_ref().is_some_and(|cursor| {
                cursor.session_id != params.session.session_id
                    || cursor.offset != 0
                    || cursor.generation != 0
            }) {
                return Err(invalid_cursor());
            }
            return Ok(AgentSessionEventsResult {
                events: Vec::new(),
                next_cursor: cursor(&params.session, 0, 0),
                has_more: false,
                active_turn: None,
            });
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, &params.session).await?;
        let after = match params.after_cursor {
            Some(after) => {
                if after.session_id != params.session.session_id || after.offset < 0 {
                    return Err(invalid_cursor());
                }
                if after.generation != generation {
                    return Err(TalosError::new(
                        ErrorCode::EventCursorExpired,
                        "Worker execution cursor belongs to a cleared Session generation.",
                    ));
                }
                if after.offset > 0 {
                    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM worker_events WHERE session_id = ? AND generation = ? AND event_offset = ?")
                        .bind(&params.session.session_id).bind(generation as i64).bind(after.offset)
                        .fetch_one(&mut *tx).await.map_err(database_error)?;
                    if exists != 1 {
                        return Err(invalid_cursor());
                    }
                }
                after.offset
            }
            None => 0,
        };
        let limit = params.limit.clamp(1, MAX_EXECUTION_PAGE_EVENTS);
        let rows: Vec<StoredExecutionEvent> = sqlx::query_as("SELECT e.event_offset, e.event_json, e.settlement_json, COALESCE(a.request_json,(SELECT first_admission.request_json FROM worker_admissions first_admission WHERE first_admission.session_id=e.session_id AND first_admission.generation=e.generation AND first_admission.turn_id=e.turn_id ORDER BY first_admission.first_offset LIMIT 1)),e.capture_json FROM worker_events e LEFT JOIN worker_admissions a ON a.session_id=e.session_id AND a.generation=e.generation AND e.event_offset BETWEEN a.first_offset AND a.last_offset WHERE e.session_id = ? AND e.generation = ? AND e.event_offset > ? ORDER BY e.event_offset LIMIT ?")
            .bind(&params.session.session_id).bind(generation as i64).bind(after).bind((limit + 1) as i64)
            .fetch_all(&mut *tx).await.map_err(database_error)?;
        let mut events = Vec::new();
        let mut bytes = 1024;
        let mut next = after;
        let mut has_more = rows.len() > limit;
        for (offset, event, settlement, admission, capture) in rows.into_iter().take(limit) {
            let event: AgentEvent = decode(&event)?;
            let admitted_command = admission
                .as_deref()
                .map(decode::<agent_protocol::AgentExecutionCommand>)
                .transpose()?;
            let execution_purpose = admitted_command
                .as_ref()
                .map(super::command_purpose)
                .ok_or_else(|| TalosError::internal("Worker event admission is missing."))?;
            let (command, automation_input, execution_model) = admitted_command
                .as_ref()
                .map(|command| super::command_metadata(command, &event))
                .unwrap_or_default();
            let record = AgentCommittedEvent {
                execution_purpose,
                history_supplement: capture.as_deref().map(decode).transpose()?,
                execution_model,
                cursor: cursor(&params.session, generation, offset),
                event,
                settlement: settlement.as_deref().map(decode).transpose()?,
                command,
                automation_input,
            };
            let size = encode(&record)?.len();
            if bytes + size > MAX_EXECUTION_PAGE_BYTES {
                has_more = true;
                break;
            }
            bytes += size;
            next = offset;
            events.push(record);
        }
        tx.commit().await.map_err(database_error)?;
        Ok(AgentSessionEventsResult {
            events,
            next_cursor: cursor(&params.session, generation, next),
            has_more,
            active_turn: active.map(|turn| AgentTurnRef::new(&params.session.session_id, turn)),
        })
    }

    /// Read an original admission only when its complete request matches.
    pub async fn admission(
        &self,
        session: &AgentSessionContext,
        identity: &AgentTurnRef,
        request: &agent_protocol::AgentExecutionCommand,
    ) -> TalosResult<Option<Vec<AgentEvent>>> {
        let input_id = super::command_key(request)?;
        let row: Option<(String, i64, i64)> = sqlx::query_as("SELECT request_json, first_offset, last_offset FROM worker_admissions WHERE session_id = ? AND generation = COALESCE((SELECT generation FROM worker_sessions WHERE session_id = ?), 0) AND input_id = ?")
            .bind(&identity.session_id).bind(&identity.session_id).bind(input_id)
            .fetch_optional(&self.pool).await.map_err(database_error)?;
        let Some((original, first, last)) = row else {
            return Ok(None);
        };
        if original != encode(request)? {
            return Err(TalosError::new(ErrorCode::IdempotencyConflict, "Worker command identity was already admitted with different input or configuration."));
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        Self::binding(&mut tx, session).await?;
        let events: Vec<String> = sqlx::query_scalar("SELECT event_json FROM worker_events WHERE session_id = ? AND event_offset >= ? AND event_offset <= ? ORDER BY event_offset")
            .bind(&identity.session_id).bind(first).bind(last).fetch_all(&mut *tx).await.map_err(database_error)?;
        tx.commit().await.map_err(database_error)?;
        events
            .iter()
            .map(|event| decode(event))
            .collect::<TalosResult<Vec<_>>>()
            .map(Some)
    }

    pub async fn is_retired(&self, session_id: &str) -> TalosResult<bool> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT retired FROM worker_sessions WHERE session_id=?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?;
        Ok(value == Some(1))
    }

    /// Retrieve an admitted scope for an exact authorized interrupt or local recovery.
    pub async fn session_scope(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentSessionContext>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT binding_json FROM worker_sessions WHERE session_id=?")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(database_error)?;
        value.as_deref().map(decode).transpose()
    }

    pub async fn turn_automation_origin(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<talos_types::AutomationInputOrigin>> {
        let request: Option<String> = sqlx::query_scalar("SELECT request_json FROM worker_admissions WHERE session_id=? AND turn_id=? AND generation=(SELECT generation FROM worker_sessions WHERE session_id=?) ORDER BY first_offset LIMIT 1").bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        match request
            .as_deref()
            .map(decode::<agent_protocol::AgentExecutionCommand>)
            .transpose()?
        {
            Some(agent_protocol::AgentExecutionCommand::Run { params }) => {
                let message_id = params.input.message_id;
                Ok(params
                    .automation_origin
                    .map(|origin| talos_types::AutomationInputOrigin { message_id, origin }))
            }
            _ => Ok(None),
        }
    }

    pub async fn read_payload(
        &self,
        params: agent_protocol::AgentExecutionPayloadParams,
    ) -> TalosResult<agent_protocol::AgentExecutionPayloadResult> {
        if params.identity.session_id != params.session.session_id || params.payload_id.is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Worker payload identity does not match Session.",
            ));
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let (generation, _) = Self::binding(&mut tx, &params.session).await?;
        let body:Option<String>=sqlx::query_scalar("SELECT payload.value FROM worker_supplements s,json_each(s.supplement_json,'$.payloads') payload WHERE s.session_id=? AND s.generation=? AND s.turn_id=? AND payload.key=? AND EXISTS (SELECT 1 FROM worker_events e WHERE e.session_id=s.session_id AND e.generation=s.generation AND e.turn_id=s.turn_id AND json_extract(e.settlement_json,'$.type')='finished')").bind(&params.session.session_id).bind(generation as i64).bind(&params.identity.turn_id).bind(&params.payload_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        let body = body.ok_or_else(|| {
            TalosError::new(
                ErrorCode::ResourceNotFound,
                "Worker execution payload is unavailable.",
            )
        })?;
        let _: talos_agent_api::trajectory::TrajectoryPayload = decode(&body)?;
        let start = usize::try_from(params.offset).map_err(|_| invalid_cursor())?;
        if start > body.len() || !body.is_char_boundary(start) {
            return Err(invalid_cursor());
        }
        let mut end = start
            .saturating_add(params.limit.clamp(4, 256 * 1024))
            .min(body.len());
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        let result = agent_protocol::AgentExecutionPayloadResult {
            data: body[start..end].to_owned(),
            next_offset: (end < body.len()).then_some(end as u64),
        };
        tx.commit().await.map_err(database_error)?;
        Ok(result)
    }

    pub async fn checkpoint(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<AgentRunCheckpoint>> {
        let value: Option<String> = sqlx::query_scalar("SELECT checkpoint_json FROM worker_checkpoints WHERE session_id = ? AND turn_id = ? AND generation = (SELECT generation FROM worker_sessions WHERE session_id = ?)")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        value.as_deref().map(decode).transpose()
    }

    pub async fn settlement(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<(AgentEvent, AgentExecutionSettlement)>> {
        let value: Option<(String, String)> = sqlx::query_as("SELECT event_json, settlement_json FROM worker_events WHERE session_id = ? AND turn_id = ? AND settlement_json IS NOT NULL AND generation = (SELECT generation FROM worker_sessions WHERE session_id = ?) ORDER BY event_offset DESC LIMIT 1")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        value
            .map(|(event, settlement)| Ok((decode(&event)?, decode(&settlement)?)))
            .transpose()
    }

    pub async fn turn_events(&self, identity: &AgentTurnRef) -> TalosResult<Vec<AgentEvent>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT event_json FROM worker_events WHERE session_id = ? AND turn_id = ? AND generation = (SELECT generation FROM worker_sessions WHERE session_id = ?) ORDER BY sequence LIMIT 10001")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_all(&self.pool).await.map_err(database_error)?;
        if rows.len() > 10000
            || rows.iter().map(String::len).sum::<usize>() > MAX_EXECUTION_PAGE_BYTES
        {
            return Err(TalosError::new(
                ErrorCode::ContextWindowExceeded,
                "Worker Turn history exceeds the bounded context read limit.",
            ));
        }
        rows.iter().map(|row| decode(row)).collect()
    }

    pub async fn terminal_events(
        &self,
        session_id: &str,
        after: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<(i64, AgentEvent)>> {
        let rows: Vec<(i64, String)> = match after {
            Some(after) => sqlx::query_as("SELECT event_offset, event_json FROM worker_events WHERE session_id = ? AND generation = (SELECT generation FROM worker_sessions WHERE session_id = ?) AND event_offset > ? AND json_extract(settlement_json, '$.type') = 'finished' ORDER BY event_offset LIMIT ?")
                .bind(session_id).bind(session_id).bind(after).bind(limit.clamp(1, 256) as i64).fetch_all(&self.pool).await,
            None => sqlx::query_as("SELECT event_offset, event_json FROM worker_events WHERE session_id = ? AND generation = (SELECT generation FROM worker_sessions WHERE session_id = ?) AND json_extract(settlement_json, '$.type') = 'finished' ORDER BY event_offset DESC LIMIT ?")
                .bind(session_id).bind(session_id).bind(limit.clamp(1, 256) as i64).fetch_all(&self.pool).await,
        }.map_err(database_error)?;
        let mut events = rows
            .into_iter()
            .map(|(seq, event)| Ok((seq, decode(&event)?)))
            .collect::<TalosResult<Vec<_>>>()?;
        events.sort_by_key(|(seq, _)| *seq);
        Ok(events)
    }

    pub async fn memory_progress(&self, session_id: &str) -> TalosResult<u64> {
        let value: Option<i64> = sqlx::query_scalar(
            "SELECT memory_processed_until FROM worker_sessions WHERE session_id = ?",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;
        value
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::SessionNotFound,
                    "Worker Memory progress Session does not exist.",
                )
            })
    }

    pub async fn supplement(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<SessionHistorySupplement>> {
        let value: Option<String> = sqlx::query_scalar("SELECT supplement_json FROM worker_supplements WHERE session_id = ? AND turn_id = ? AND generation = (SELECT generation FROM worker_sessions WHERE session_id = ?)")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        value.as_deref().map(decode).transpose()
    }
}

fn invalid_cursor() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidCursor,
        "Worker execution cursor is invalid for this Session.",
    )
}
