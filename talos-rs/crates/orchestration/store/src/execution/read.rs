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
        let rows: Vec<(String, String, String)> = sqlx::query_as("SELECT s.binding_json, s.active_turn_id, c.checkpoint_json FROM sessions s JOIN turn_checkpoints c ON c.session_id=s.session_id AND c.generation=s.generation AND c.turn_id=s.active_turn_id WHERE s.active_turn_id IS NOT NULL AND COALESCE((SELECT json_extract(e.settlement_json, '$.type') FROM agent_events e WHERE e.session_id=s.session_id AND e.generation=s.generation AND e.turn_id=s.active_turn_id ORDER BY e.event_offset DESC LIMIT 1), '') <> 'waitingForInput' LIMIT 256")
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
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_id = ?")
            .bind(&session.session_id)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        if exists == 0 {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        let terminal: Option<String> = sqlx::query_scalar("SELECT event_json FROM agent_events WHERE session_id = ? AND generation = ? AND json_extract(settlement_json, '$.type') = 'finished' ORDER BY event_offset DESC LIMIT 1").bind(&session.session_id).bind(generation as i64).fetch_optional(&mut *tx).await.map_err(database_error)?;
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
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE session_id=?")
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
                    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events WHERE session_id = ? AND generation = ? AND event_offset = ?")
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
        let rows: Vec<StoredExecutionEvent> = sqlx::query_as("SELECT e.event_offset, e.event_json, e.settlement_json, COALESCE(a.request_json,(SELECT first_admission.request_json FROM turn_admissions first_admission WHERE first_admission.session_id=e.session_id AND first_admission.generation=e.generation AND first_admission.turn_id=e.turn_id ORDER BY first_admission.first_offset LIMIT 1)),e.capture_json FROM agent_events e LEFT JOIN turn_admissions a ON a.session_id=e.session_id AND a.generation=e.generation AND e.event_offset BETWEEN a.first_offset AND a.last_offset WHERE e.session_id = ? AND e.generation = ? AND e.event_offset > ? ORDER BY e.event_offset LIMIT ?")
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
        let row: Option<(String, i64, i64)> = sqlx::query_as("SELECT request_json, first_offset, last_offset FROM turn_admissions WHERE session_id = ? AND generation = COALESCE((SELECT generation FROM sessions WHERE session_id = ?), 0) AND input_id = ?")
            .bind(&identity.session_id).bind(&identity.session_id).bind(input_id)
            .fetch_optional(&self.pool).await.map_err(database_error)?;
        let Some((original, first, last)) = row else {
            return Ok(None);
        };
        if decode::<agent_protocol::AgentExecutionCommand>(&original)? != *request {
            return Err(TalosError::new(ErrorCode::IdempotencyConflict, "Worker command identity was already admitted with different input or configuration."));
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        Self::binding(&mut tx, session).await?;
        let events: Vec<String> = sqlx::query_scalar("SELECT event_json FROM agent_events WHERE session_id = ? AND event_offset >= ? AND event_offset <= ? ORDER BY event_offset")
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
            sqlx::query_scalar("SELECT retired FROM sessions WHERE session_id=?")
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
            sqlx::query_scalar("SELECT binding_json FROM sessions WHERE session_id=?")
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
        let row: Option<(Option<String>, Option<String>, i64)> = sqlx::query_as("SELECT CASE WHEN length(CAST(json_extract(request_json,'$.params.automation_origin') AS BLOB))<=? THEN json_extract(request_json,'$.params.automation_origin') END, CASE WHEN length(CAST(json_extract(request_json,'$.params.input.messageId') AS BLOB))<=? THEN json_extract(request_json,'$.params.input.messageId') END, COALESCE(length(CAST(json_extract(request_json,'$.params.automation_origin') AS BLOB)),0)+COALESCE(length(CAST(json_extract(request_json,'$.params.input.messageId') AS BLOB)),0) FROM turn_admissions WHERE session_id=? AND turn_id=? AND generation=(SELECT generation FROM sessions WHERE session_id=?) ORDER BY first_offset LIMIT 1")
            .bind(agent_protocol::MAX_EXECUTION_EVENT_BYTES as i64).bind(agent_protocol::MAX_EXECUTION_EVENT_BYTES as i64).bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        match row {
            Some((_, _, bytes)) if bytes as usize > agent_protocol::MAX_EXECUTION_EVENT_BYTES => {
                Err(TalosError::new(
                    ErrorCode::ContextWindowExceeded,
                    "Worker history automation metadata exceeds its bounded read limit.",
                ))
            }
            Some((Some(origin), Some(message_id), _)) => {
                Ok(Some(talos_types::AutomationInputOrigin {
                    message_id,
                    origin: decode(&origin)?,
                }))
            }
            Some((Some(_), None, _)) => Err(TalosError::internal(
                "Worker history automation message identity is missing.",
            )),
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
        let body:Option<String>=sqlx::query_scalar("SELECT payload.value FROM session_history_supplements s,json_each(s.supplement_json,'$.payloads') payload WHERE s.session_id=? AND s.generation=? AND s.turn_id=? AND payload.key=? AND EXISTS (SELECT 1 FROM agent_events e WHERE e.session_id=s.session_id AND e.generation=s.generation AND e.turn_id=s.turn_id AND json_extract(e.settlement_json,'$.type')='finished')").bind(&params.session.session_id).bind(generation as i64).bind(&params.identity.turn_id).bind(&params.payload_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
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
        let value: Option<String> = sqlx::query_scalar("SELECT checkpoint_json FROM turn_checkpoints WHERE session_id = ? AND turn_id = ? AND generation = (SELECT generation FROM sessions WHERE session_id = ?)")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        value.as_deref().map(decode).transpose()
    }

    /// Resolve a Provider reference only for the current locally committed waiting baseline.
    pub async fn waiting_continuation(
        &self,
        session: &AgentSessionContext,
        identity: &AgentTurnRef,
        waiting_sequence: u64,
    ) -> TalosResult<talos_agent_api::AgentContinuationRef> {
        if session.session_id != identity.session_id {
            return Err(super::conflict());
        }
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        if active.as_deref() != Some(identity.turn_id.as_str()) {
            return Err(super::conflict());
        }
        let value: Option<(String, String, String, String)> = sqlx::query_as("SELECT c.provider_continuation_json,c.checkpoint_json,e.event_json,e.settlement_json FROM turn_checkpoints c JOIN agent_events e ON e.session_id=c.session_id AND e.generation=c.generation AND e.turn_id=c.turn_id AND e.sequence=c.waiting_sequence WHERE c.session_id=? AND c.generation=? AND c.turn_id=? AND c.waiting_sequence=? AND c.provider_continuation_json IS NOT NULL AND e.settlement_json IS NOT NULL")
            .bind(&session.session_id).bind(generation as i64).bind(&identity.turn_id).bind(i64::try_from(waiting_sequence).map_err(|_| super::conflict())?)
            .fetch_optional(&mut *tx).await.map_err(database_error)?;
        let (reference, checkpoint, event, settlement) = value.ok_or_else(|| {
            TalosError::new(
                ErrorCode::Conflict,
                "Worker private continuation does not match its committed waiting state.",
            )
        })?;
        let checkpoint: AgentRunCheckpoint = decode(&checkpoint)?;
        let event: AgentEvent = decode(&event)?;
        let settlement: AgentExecutionSettlement = decode(&settlement)?;
        let reference: talos_agent_api::AgentContinuationRef = decode(&reference)?;
        if checkpoint.last_sequence != waiting_sequence
            || !matches!(
                event.payload,
                talos_agent_api::AgentEventPayload::WaitingForInput(_)
            )
            || !matches!(settlement, AgentExecutionSettlement::WaitingForInput { .. })
            || reference.opaque_id.trim().is_empty()
        {
            return Err(super::conflict());
        }
        tx.commit().await.map_err(database_error)?;
        Ok(reference)
    }

    pub async fn settlement(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<(AgentEvent, AgentExecutionSettlement)>> {
        let value: Option<(String, String)> = sqlx::query_as("SELECT event_json, settlement_json FROM agent_events WHERE session_id = ? AND turn_id = ? AND settlement_json IS NOT NULL AND generation = (SELECT generation FROM sessions WHERE session_id = ?) ORDER BY event_offset DESC LIMIT 1")
            .bind(&identity.session_id).bind(&identity.turn_id).bind(&identity.session_id).fetch_optional(&self.pool).await.map_err(database_error)?;
        value
            .map(|(event, settlement)| Ok((decode(&event)?, decode(&settlement)?)))
            .transpose()
    }

    pub async fn turn_events(&self, identity: &AgentTurnRef) -> TalosResult<Vec<AgentEvent>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT event_json FROM agent_events WHERE session_id = ? AND turn_id = ? AND generation = (SELECT generation FROM sessions WHERE session_id = ?) ORDER BY sequence LIMIT 10001")
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
        let direction = if after.is_some() { "ASC" } else { "DESC" };
        let query = format!("WITH candidates AS (SELECT event_offset,length(CAST(event_json AS BLOB)) AS bytes FROM agent_events WHERE session_id=? AND generation=(SELECT generation FROM sessions WHERE session_id=?) AND event_offset>? AND json_extract(settlement_json,'$.type')='finished' ORDER BY event_offset {direction} LIMIT ?), bounded AS (SELECT *,sum(bytes) OVER (ORDER BY event_offset {direction}) AS total FROM candidates) SELECT bounded.event_offset,CASE WHEN bytes<=? AND total<=? THEN body.event_json END,bytes FROM bounded JOIN agent_events body ON body.event_offset=bounded.event_offset ORDER BY bounded.event_offset {direction}");
        let rows: Vec<(i64, Option<String>, i64)> = sqlx::query_as(&query)
            .bind(session_id)
            .bind(session_id)
            .bind(after.unwrap_or_default())
            .bind(limit.clamp(1, 256) as i64)
            .bind(agent_protocol::MAX_EXECUTION_EVENT_BYTES as i64)
            .bind(MAX_EXECUTION_PAGE_BYTES as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        let mut events = Vec::new();
        for (sequence, body, bytes) in rows {
            if bytes as usize > agent_protocol::MAX_EXECUTION_EVENT_BYTES {
                return Err(TalosError::new(
                    ErrorCode::ContextWindowExceeded,
                    "Worker terminal history exceeds its bounded item limit.",
                ));
            }
            let Some(body) = body else {
                break;
            };
            events.push((sequence, decode(&body)?));
        }
        events.sort_by_key(|(seq, _)| *seq);
        Ok(events)
    }

    pub async fn memory_progress(&self, session_id: &str) -> TalosResult<u64> {
        let value: Option<i64> =
            sqlx::query_scalar("SELECT memory_processed_until FROM sessions WHERE session_id = ?")
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
        let value: Option<String> = sqlx::query_scalar("SELECT supplement_json FROM session_history_supplements WHERE session_id = ? AND turn_id = ? AND generation = (SELECT generation FROM sessions WHERE session_id = ?)")
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
