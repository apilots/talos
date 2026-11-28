//! Retained title execution facts, scoped to the local successful Turn generation.
use super::*;
use agent_protocol::{AgentSessionTitleParams, AgentSessionTitleResult};

#[derive(serde::Serialize)]
pub struct ExecutionTitleText {
    pub user_message: String,
    pub assistant_response: String,
}

impl WorkerExecutionStore {
    /// Return only the title prompt's existing 400-character prefixes from local facts.
    /// Large Tool bodies and message suffixes never enter the title execution process.
    pub async fn session_title_text(
        &self,
        params: &AgentSessionTitleParams,
    ) -> TalosResult<ExecutionTitleText> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        Self::title_generation(&mut tx, params).await?;
        let (_, active) = Self::binding(&mut tx, &params.session).await?;
        if active.is_some() {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Session title generation requires an idle Worker Session.",
            ));
        }

        let rows: Vec<(String, String)> = sqlx::query_as(r#"
            WITH starts AS (
                SELECT json_extract(event_json,'$.payload.data.message.messageId') AS message_id,min(event_offset) AS position
                FROM agent_events WHERE session_id=? AND turn_id=? AND generation=(SELECT generation FROM sessions WHERE session_id=?)
                    AND json_extract(event_json,'$.payload.type')='messageStarted' GROUP BY message_id
            ), blocks AS (
                SELECT coalesce(starts.position,e.event_offset) AS position,CAST(b.key AS INTEGER) AS block_position,
                    json_extract(b.value,'$.content.type') AS kind,substr(json_extract(b.value,'$.content.data.text'),1,400) AS text
                FROM agent_events e LEFT JOIN starts ON starts.message_id=json_extract(e.event_json,'$.payload.data.messageId'),
                    json_each(e.event_json,'$.payload.data.blocks') b
                WHERE e.session_id=? AND e.turn_id=? AND e.generation=(SELECT generation FROM sessions WHERE session_id=?)
                    AND json_extract(e.event_json,'$.payload.type')='messageCompleted'
                    AND (json_extract(b.value,'$.content.type')='assistantText' OR
                        (json_extract(b.value,'$.content.type')='userContent' AND json_extract(b.value,'$.content.data.type')='text'))
            ), prefixes AS (
                SELECT *,coalesce(sum(length(text)) OVER (PARTITION BY kind ORDER BY position,block_position ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING),0) AS preceding
                FROM blocks
            ) SELECT kind,substr(text,1,400-preceding) FROM prefixes WHERE preceding<400 AND length(text)>0 ORDER BY position,block_position
        "#)
            .bind(&params.session.session_id).bind(&params.identity.turn_id).bind(&params.session.session_id)
            .bind(&params.session.session_id).bind(&params.identity.turn_id).bind(&params.session.session_id)
            .fetch_all(&mut *tx).await.map_err(database_error)?;
        let mut input = ExecutionTitleText {
            user_message: String::new(),
            assistant_response: String::new(),
        };
        for (kind, text) in rows {
            if kind == "userContent" {
                input.user_message.push_str(&text);
            } else {
                input.assistant_response.push_str(&text);
            }
        }
        Ok(input)
    }

    async fn title_generation(
        tx: &mut Transaction<'_, Sqlite>,
        params: &AgentSessionTitleParams,
    ) -> TalosResult<u64> {
        if params.identity.session_id != params.session.session_id {
            return Err(conflict());
        }
        let (generation, _) = Self::binding(tx, &params.session).await?;
        let completed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_events e JOIN sessions s ON s.session_id=e.session_id AND s.generation=e.generation WHERE e.session_id=? AND e.turn_id=? AND s.retired=0 AND json_extract(e.event_json,'$.payload.type')='turnCompleted' AND e.settlement_json IS NOT NULL")
            .bind(&params.session.session_id).bind(&params.identity.turn_id).fetch_one(&mut **tx).await.map_err(database_error)?;
        if completed != 1 {
            return Err(conflict());
        }
        Ok(generation)
    }
    pub async fn read_session_title(
        &self,
        params: &AgentSessionTitleParams,
    ) -> TalosResult<Option<AgentSessionTitleResult>> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let generation = Self::title_generation(&mut tx, params).await?;
        let json: Option<String> = sqlx::query_scalar("SELECT result_json FROM session_title_results WHERE session_id=? AND generation=? AND turn_id=?")
            .bind(&params.session.session_id).bind(generation as i64).bind(&params.identity.turn_id).fetch_optional(&mut *tx).await.map_err(database_error)?;
        json.map(|json| decode(&json)).transpose()
    }
    pub async fn save_session_title(
        &self,
        params: &AgentSessionTitleParams,
        result: &AgentSessionTitleResult,
    ) -> TalosResult<()> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // Reserve the writer before reading its binding, as for execution append.
        sqlx::query("UPDATE sessions SET generation=generation WHERE session_id=?")
            .bind(&params.session.session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let generation = Self::title_generation(&mut tx, params).await?;
        sqlx::query("INSERT INTO session_title_results(session_id,generation,turn_id,result_json) VALUES(?,?,?,?)")
            .bind(&params.session.session_id).bind(generation as i64).bind(&params.identity.turn_id).bind(encode(result)?).execute(&mut *tx).await.map_err(database_error)?;
        tx.commit().await.map_err(database_error)
    }
}
