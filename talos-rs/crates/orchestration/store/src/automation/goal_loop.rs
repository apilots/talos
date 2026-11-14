//! Durable Session definitions and metadata attached to an already admitted Turn.
//! No queue, occurrence ledger, retry engine or execution ownership lives here.
//! SQL rows and normalization remain private in records; public operations reuse
//! the canonical typed domain values, never exposing row/cursor/transaction data.
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use talos_agent_api::{AgentEventPayload, AgentMessageBlockContent, AgentTurnRef, AgentUserInput};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_storage::{Row, Storage, StorageTransaction};
use talos_types::{
    AutomationTurnOrigin, GoalStatus, LoopStatus, SessionAutomationSnapshot, SessionGoal,
    SessionLoop,
};

mod admission;
mod records;
use admission::bind;
pub(crate) use admission::bind_admission_origin;
use records::{goal_from_row, loop_from_row, write_goal, write_loop};

#[derive(Clone)]
pub struct DatabaseAutomationStore {
    storage: Arc<dyn Storage>,
}
impl DatabaseAutomationStore {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }
    pub async fn snapshot(&self, session_id: &str) -> TalosResult<SessionAutomationSnapshot> {
        let mut tx = self.storage.begin_read_transaction().await?;
        let goal = tx.query("SELECT * FROM session_goals WHERE session_id=? AND status IN ('active','paused','complete') ORDER BY CASE WHEN status IN ('active','paused') THEN 0 ELSE 1 END,created_at DESC,id DESC LIMIT 1", &[session_id]).await?.first().map(goal_from_row).transpose()?;
        let loops = tx.query("SELECT * FROM session_loops WHERE session_id=? AND status='active' AND cleared_at IS NULL ORDER BY created_at,id", &[session_id]).await?.iter().map(loop_from_row).collect::<TalosResult<_>>()?;
        tx.rollback().await?;
        Ok(SessionAutomationSnapshot {
            session_id: session_id.into(),
            goal,
            loops,
        })
    }
    pub async fn goal(&self, id: &str) -> TalosResult<SessionGoal> {
        self.storage
            .query("SELECT * FROM session_goals WHERE id=?", &[id])
            .await?
            .first()
            .map(goal_from_row)
            .transpose()?
            .ok_or_else(missing)
    }
    pub async fn loop_task(&self, id: &str) -> TalosResult<SessionLoop> {
        self.storage
            .query(
                "SELECT * FROM session_loops WHERE id=? AND cleared_at IS NULL",
                &[id],
            )
            .await?
            .first()
            .map(loop_from_row)
            .transpose()?
            .ok_or_else(missing)
    }
    pub async fn goals(&self, session_id: &str) -> TalosResult<Vec<SessionGoal>> {
        self.storage
            .query(
                "SELECT * FROM session_goals WHERE session_id=? ORDER BY created_at,id",
                &[session_id],
            )
            .await?
            .iter()
            .map(goal_from_row)
            .collect()
    }
    pub async fn loops(&self, session_id: &str) -> TalosResult<Vec<SessionLoop>> {
        self.storage.query("SELECT * FROM session_loops WHERE session_id=? AND cleared_at IS NULL ORDER BY created_at,id", &[session_id]).await?.iter().map(loop_from_row).collect()
    }
    /// Bounded keyset pagination across original Session-owned definitions.
    /// Actor identity is a trusted Service input, never a request-owned user ID.
    pub async fn list(
        &self,
        user_id: &str,
        request: &talos_types::AutomationListRequest,
    ) -> TalosResult<talos_types::AutomationListResponse> {
        let ended = request.scope == talos_types::AutomationListScope::Ended;
        let goal_filter = if ended {
            "g.status='complete'"
        } else {
            "g.status IN ('active','paused')"
        };
        let loop_filter = if ended {
            "l.status='stopped'"
        } else {
            "l.status='active'"
        };
        let session = request.session_id.as_deref().unwrap_or("");
        let cursor = request.cursor.as_deref().unwrap_or("");
        let limit = request.limit.unwrap_or(50).clamp(1, 200) as usize;
        let sql=format!("SELECT * FROM (SELECT g.id,'goal' AS kind FROM session_goals g JOIN sessions s ON s.session_key=g.session_id WHERE s.user_id=? AND NULLIF(s.tree_root_session_id,'') IS NULL AND (?='' OR g.session_id=?) AND {goal_filter} UNION ALL SELECT l.id,'loop' AS kind FROM session_loops l JOIN sessions s ON s.session_key=l.session_id WHERE s.user_id=? AND NULLIF(s.tree_root_session_id,'') IS NULL AND (?='' OR l.session_id=?) AND {loop_filter} AND l.cleared_at IS NULL) WHERE id>? ORDER BY id LIMIT ?");
        let mut tx = self.storage.begin_read_transaction().await?;
        let rows = tx
            .query(
                &sql,
                &[
                    user_id,
                    session,
                    session,
                    user_id,
                    session,
                    session,
                    cursor,
                    &(limit + 1).to_string(),
                ],
            )
            .await?;
        let mut data = Vec::with_capacity(rows.len().min(limit));
        for row in rows.iter().take(limit) {
            let id = row.get_string("id").ok_or_else(missing)?;
            if row.get_string("kind").as_deref() == Some("goal") {
                let values = tx
                    .query("SELECT * FROM session_goals WHERE id=?", &[&id])
                    .await?;
                data.push(talos_types::AutomationListEntry::Goal {
                    goal: goal_from_row(values.first().ok_or_else(missing)?)?,
                });
            } else {
                let values = tx
                    .query("SELECT * FROM session_loops WHERE id=?", &[&id])
                    .await?;
                data.push(talos_types::AutomationListEntry::Loop {
                    loop_task: loop_from_row(values.first().ok_or_else(missing)?)?,
                });
            }
        }
        let next_cursor = if rows.len() > limit {
            rows[limit - 1].get_string("id")
        } else {
            None
        };
        tx.rollback().await?;
        Ok(talos_types::AutomationListResponse { data, next_cursor })
    }
    pub async fn active_loop_sessions(&self) -> TalosResult<Vec<String>> {
        Ok(self.storage.query("SELECT DISTINCT session_id FROM session_loops WHERE status='active' AND cleared_at IS NULL", &[]).await?.iter().filter_map(|r| r.get_string("session_id")).collect())
    }
    pub async fn active_goal_sessions(&self) -> TalosResult<Vec<String>> {
        Ok(self
            .storage
            .query(
                "SELECT DISTINCT session_id FROM session_goals WHERE status='active'",
                &[],
            )
            .await?
            .iter()
            .filter_map(|row| row.get_string("session_id"))
            .collect())
    }
    /// Only durable, never-admitted Goals need an initial clock wake. Ongoing
    /// Goals still progress solely through original settlement/reconciliation.
    pub async fn initial_goal_sessions(&self, now: i64) -> TalosResult<Vec<String>> {
        let before = now
            .saturating_sub(talos_types::AUTOMATION_INITIAL_DELAY_SECONDS)
            .to_string();
        Ok(self.storage.query("SELECT DISTINCT session_id FROM session_goals WHERE status='active' AND latest_turn_id IS NULL AND created_at<=?", &[&before]).await?.iter().filter_map(|row| row.get_string("session_id")).collect())
    }
    pub async fn create_goal(
        &self,
        goal: &SessionGoal,
        turn: Option<&AgentTurnRef>,
    ) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        ensure_session(tx.as_mut(), &goal.session_id).await?;
        ensure_no_mode(
            tx.as_mut(),
            &goal.session_id,
            "session_loops",
            "status='active'",
        )
        .await?;
        ensure_no_mode(
            tx.as_mut(),
            &goal.session_id,
            "session_goals",
            "status IN ('active','paused')",
        )
        .await?;
        write_goal(tx.as_mut(), goal, records::Write::Insert).await?;
        if let Some(turn) = turn {
            bind(
                tx.as_mut(),
                turn,
                &AutomationTurnOrigin::Goal {
                    goal_id: goal.id.clone(),
                    objective_revision: goal.objective_revision,
                },
            )
            .await?;
        }
        tx.commit().await
    }
    pub async fn create_loop(&self, task: &SessionLoop) -> TalosResult<()> {
        self.insert_loop(task, None).await.map(|_| ())
    }
    /// Trusted original Tool call identity, not a model argument or occurrence ledger.
    pub async fn create_loop_for_call(
        &self,
        task: &SessionLoop,
        turn: &AgentTurnRef,
        call_id: &str,
    ) -> TalosResult<SessionLoop> {
        if call_id.trim().is_empty() || turn.session_id != task.session_id {
            return Err(conflict());
        }
        self.insert_loop(task, Some((turn, call_id))).await
    }
    async fn insert_loop(
        &self,
        task: &SessionLoop,
        call: Option<(&AgentTurnRef, &str)>,
    ) -> TalosResult<SessionLoop> {
        task.schedule.validate()?;
        let mut tx = self.storage.begin_transaction().await?;
        ensure_session(tx.as_mut(), &task.session_id).await?;
        if let Some((turn, call_id)) = call {
            let rows=tx.query("SELECT * FROM session_loops WHERE session_id=? AND activation_turn_id=? AND activation_call_id=?", &[&task.session_id,&turn.turn_id,call_id]).await?;
            if let Some(row) = rows.first() {
                let original = loop_from_row(row)?;
                tx.rollback().await?;
                return Ok(original);
            }
        }
        if let Some((turn, _)) = call {
            if tx.query("SELECT initial_message_id FROM agent_active_turns WHERE session_id=? AND turn_id=? AND state='running'", &[&turn.session_id,&turn.turn_id]).await?.is_empty(){return Err(conflict())}
        }
        ensure_no_mode(
            tx.as_mut(),
            &task.session_id,
            "session_goals",
            "status IN ('active','paused')",
        )
        .await?;
        write_loop(tx.as_mut(), task, records::Write::Insert).await?;
        if let Some((turn, call_id)) = call {
            tx.execute(
                "UPDATE session_loops SET activation_turn_id=?,activation_call_id=? WHERE id=?",
                &[&turn.turn_id, call_id, &task.id],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(task.clone())
    }
    /// Compare the complete previous typed value in one writer transaction. State
    /// operations do not increment objective/config revisions used by model fences.
    pub async fn replace_goal(
        &self,
        previous: &SessionGoal,
        next: &SessionGoal,
    ) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        let current = tx
            .query("SELECT * FROM session_goals WHERE id=?", &[&previous.id])
            .await?
            .first()
            .map(goal_from_row)
            .transpose()?
            .ok_or_else(missing)?;
        if current != *previous || next.id != current.id || next.session_id != current.session_id {
            return Err(conflict());
        }
        if matches!(next.status, GoalStatus::Active | GoalStatus::Paused) {
            ensure_no_mode(
                tx.as_mut(),
                &next.session_id,
                "session_loops",
                "status='active'",
            )
            .await?;
        }
        write_goal(tx.as_mut(), next, records::Write::Update).await?;
        tx.commit().await
    }
    pub async fn replace_loop(
        &self,
        previous: &SessionLoop,
        next: &SessionLoop,
    ) -> TalosResult<()> {
        next.schedule.validate()?;
        let mut tx = self.storage.begin_transaction().await?;
        let current = tx
            .query(
                "SELECT * FROM session_loops WHERE id=? AND cleared_at IS NULL",
                &[&previous.id],
            )
            .await?
            .first()
            .map(loop_from_row)
            .transpose()?
            .ok_or_else(missing)?;
        if current != *previous || next.id != current.id || next.session_id != current.session_id {
            return Err(conflict());
        }
        if next.status == LoopStatus::Active {
            ensure_no_mode(
                tx.as_mut(),
                &next.session_id,
                "session_goals",
                "status IN ('active','paused')",
            )
            .await?;
        }
        write_loop(tx.as_mut(), next, records::Write::Update).await?;
        tx.commit().await
    }
    pub async fn clear_loop(&self, previous: &SessionLoop) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        let current = tx
            .query(
                "SELECT * FROM session_loops WHERE id=? AND cleared_at IS NULL",
                &[&previous.id],
            )
            .await?
            .first()
            .map(loop_from_row)
            .transpose()?
            .ok_or_else(missing)?;
        if current != *previous {
            return Err(conflict());
        }
        tx.execute("UPDATE session_loops SET cleared_at=?,status='stopped',stop_reason='userStopped',next_due_at=NULL WHERE id=?", &[&chrono::Utc::now().timestamp().to_string(),&previous.id]).await?;
        tx.commit().await
    }
    pub async fn bind_origin(
        &self,
        turn: &AgentTurnRef,
        origin: &AutomationTurnOrigin,
    ) -> TalosResult<()> {
        let mut tx = self.storage.begin_transaction().await?;
        bind(tx.as_mut(), turn, origin).await?;
        tx.commit().await
    }
    pub async fn origin(&self, turn: &AgentTurnRef) -> TalosResult<Option<AutomationTurnOrigin>> {
        let rows = self.storage.query("SELECT automation_origin_json FROM agent_events WHERE session_id=? AND turn_id=? AND sequence=1", &[&turn.session_id, &turn.turn_id]).await?;
        rows.first()
            .and_then(|r| r.get_string("automation_origin_json"))
            .map(|raw| decode(&raw))
            .transpose()
    }
    /// A Loop registration is recorded by its creating Tool call, not an
    /// automatic-work origin. Clear preserves this receipt on the original row.
    pub async fn has_loop_activation(&self, turn: &AgentTurnRef) -> TalosResult<bool> {
        Ok(!self.storage.query("SELECT 1 WHERE EXISTS(SELECT 1 FROM session_loops WHERE session_id=? AND activation_turn_id=?)", &[&turn.session_id, &turn.turn_id]).await?.is_empty())
    }
    /// Read the original canonical User input after terminal settlement has
    /// released the active Turn. Later Steer messages must not replace it.
    pub async fn initial_user_input(
        &self,
        turn: &AgentTurnRef,
    ) -> TalosResult<Option<AgentUserInput>> {
        let rows = self.storage.query("SELECT payload_json FROM agent_events WHERE session_id=? AND turn_id=? AND sequence IS NOT NULL AND json_extract(payload_json,'$.type')='messageCompleted' AND json_extract(payload_json,'$.data.role')='user' ORDER BY sequence LIMIT 1", &[&turn.session_id, &turn.turn_id]).await?;
        rows.first()
            .map(|row| {
                let payload = row.get_string("payload_json").ok_or_else(missing)?;
                match decode::<AgentEventPayload>(&payload)? {
                    AgentEventPayload::MessageCompleted(message) => Ok(AgentUserInput {
                        content: message
                            .blocks
                            .into_iter()
                            .filter_map(|block| match block.content {
                                AgentMessageBlockContent::UserContent(content) => Some(content),
                                _ => None,
                            })
                            .collect(),
                    }),
                    _ => Err(conflict()),
                }
            })
            .transpose()
    }
    /// Exact triggering User input only; later Steer messages share the logical
    /// Turn but never inherit its Input-footer badge.
    pub async fn input_origin(
        &self,
        turn: &AgentTurnRef,
    ) -> TalosResult<Option<talos_types::AutomationInputOrigin>> {
        Ok(self
            .input_origins(std::slice::from_ref(turn))
            .await?
            .into_iter()
            .next()
            .map(|(_, value)| value))
    }
    pub async fn input_origins(
        &self,
        turns: &[AgentTurnRef],
    ) -> TalosResult<Vec<(AgentTurnRef, talos_types::AutomationInputOrigin)>> {
        if turns.len() > 1000 {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Automation origin queries cannot exceed 1,000 Turns.",
            ));
        }
        let requested = encode(&turns)?;
        let rows=self.storage.query("SELECT session_id,turn_id,automation_input_message_id,automation_origin_json FROM agent_events WHERE sequence=1 AND automation_origin_json IS NOT NULL AND (session_id,turn_id) IN (SELECT json_extract(value,'$.sessionId'),json_extract(value,'$.turnId') FROM json_each(?))", &[&requested]).await?;
        rows.iter()
            .map(|row| {
                Ok((
                    AgentTurnRef::new(
                        row.get_string("session_id").ok_or_else(missing)?,
                        row.get_string("turn_id").ok_or_else(missing)?,
                    ),
                    talos_types::AutomationInputOrigin {
                        message_id: row
                            .get_string("automation_input_message_id")
                            .ok_or_else(missing)?,
                        origin: decode(
                            &row.get_string("automation_origin_json")
                                .ok_or_else(missing)?,
                        )?,
                    },
                ))
            })
            .collect()
    }
    /// A later canonical text/file User message is the existing committed Steer
    /// fact. An approval/question response by itself is not management input.
    pub async fn has_user_steer(&self, turn: &AgentTurnRef) -> TalosResult<bool> {
        Ok(!self.storage.query("SELECT 1 FROM agent_events e JOIN agent_events b ON b.session_id=e.session_id AND b.turn_id=e.turn_id AND b.sequence=1 WHERE e.session_id=? AND e.turn_id=? AND json_extract(e.payload_json,'$.type')='messageCompleted' AND json_extract(e.payload_json,'$.data.role')='user' AND json_extract(e.payload_json,'$.data.messageId')<>b.automation_input_message_id AND EXISTS (SELECT 1 FROM json_each(e.payload_json,'$.data.blocks') block WHERE json_extract(block.value,'$.content.type')='userContent' AND json_extract(block.value,'$.content.data.type') IN ('text','file')) LIMIT 1", &[&turn.session_id,&turn.turn_id]).await?.is_empty())
    }
    /// Only already persisted metrics may contribute after recovery. Absence is
    /// unknown, never proof of zero work or a missing model declaration.
    pub async fn observed_metrics(
        &self,
        turn: &AgentTurnRef,
    ) -> TalosResult<Option<talos_agent_api::AgentRunMetrics>> {
        let rows=self.storage.query("SELECT payload_json FROM agent_events WHERE session_id=? AND turn_id=? AND sequence IS NOT NULL AND json_extract(payload_json,'$.type')='metricsUpdated' ORDER BY event_seq DESC LIMIT 1", &[&turn.session_id,&turn.turn_id]).await?;
        match rows.first().and_then(|r| r.get_string("payload_json")) {
            Some(raw) => match decode::<talos_agent_api::AgentEventPayload>(&raw)? {
                talos_agent_api::AgentEventPayload::MetricsUpdated(metrics) => Ok(Some(metrics)),
                _ => Err(conflict()),
            },
            None => Ok(None),
        }
    }
    /// Each logical Turn reports cumulative observed usage. CAS the original
    /// metadata watermark and add only its positive delta, including failures.
    /// Late usage must not move a completed Goal's updated_at duration endpoint.
    pub async fn account_usage(
        &self,
        turn: &AgentTurnRef,
        tokens: u64,
        now: i64,
    ) -> TalosResult<()> {
        let tokens = i64::try_from(tokens).map_err(|_| conflict())?;
        let mut tx = self.storage.begin_transaction().await?;
        let rows = tx.query("SELECT automation_origin_json,automation_accounted_tokens FROM agent_events WHERE session_id=? AND turn_id=? AND sequence=1", &[&turn.session_id, &turn.turn_id]).await?;
        if let Some(row) = rows.first() {
            let previous = row.get_i64("automation_accounted_tokens").unwrap_or(0);
            if tokens > previous {
                if let Some(raw) = row.get_string("automation_origin_json") {
                    if let AutomationTurnOrigin::Goal { goal_id, .. } = decode(&raw)? {
                        tx.execute("UPDATE session_goals SET tokens_used=tokens_used+?,updated_at=CASE WHEN status='complete' THEN updated_at ELSE ? END WHERE id=? AND session_id=?", &[&(tokens-previous).to_string(), &now.to_string(), &goal_id, &turn.session_id]).await?;
                    }
                    tx.execute("UPDATE agent_events SET automation_accounted_tokens=? WHERE session_id=? AND turn_id=? AND sequence=1 AND automation_accounted_tokens=?", &[&tokens.to_string(), &turn.session_id, &turn.turn_id, &previous.to_string()]).await?;
                }
            }
        }
        tx.commit().await
    }
}
async fn ensure_session(tx: &mut dyn StorageTransaction, id: &str) -> TalosResult<()> {
    if tx
        .query(
            "SELECT session_key FROM sessions WHERE session_key=?",
            &[id],
        )
        .await?
        .is_empty()
    {
        return Err(missing());
    }
    Ok(())
}
async fn ensure_no_mode(
    tx: &mut dyn StorageTransaction,
    session: &str,
    table: &str,
    filter: &str,
) -> TalosResult<()> {
    // Table/filter are private compile-time call sites, never caller parameters.
    if !tx
        .query(
            &format!("SELECT id FROM {table} WHERE session_id=? AND {filter} LIMIT 1"),
            &[session],
        )
        .await?
        .is_empty()
    {
        return Err(conflict());
    }
    Ok(())
}
fn encode<T: Serialize>(value: &T) -> TalosResult<String> {
    serde_json::to_string(value)
        .map_err(|e| TalosError::internal_with_source("Could not encode automation data.", e))
}
fn decode<T: DeserializeOwned>(raw: &str) -> TalosResult<T> {
    serde_json::from_str(raw)
        .map_err(|e| TalosError::internal_with_source("Invalid stored automation data.", e))
}
fn missing() -> TalosError {
    TalosError::new(
        ErrorCode::ResourceNotFound,
        "Automation object or Turn was not found.",
    )
}
fn conflict() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "Session automation changed or conflicts with its current mode.",
    )
}
#[cfg(test)]
#[path = "goal_loop_tests.rs"]
mod tests;
