//! Retained background maintenance facts; product accounting stays in the product Store.

use super::*;
use agent_protocol::{
    DreamMemoryCursor, DreamMemoryFormation, DreamMemoryParams, DreamMemoryResult,
};

impl WorkerExecutionStore {
    pub async fn read_dream_formation(
        &self,
        params: &DreamMemoryParams,
    ) -> TalosResult<DreamMemoryResult> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, &params.session).await?;
        let (retired, processed_until): (i64, i64) = sqlx::query_as(
            "SELECT retired,memory_processed_until FROM sessions WHERE session_id=?",
        )
        .bind(&params.session.session_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;
        if retired != 0 {
            return Err(TalosError::new(
                ErrorCode::SessionNotFound,
                "Worker Session was permanently deleted.",
            ));
        }
        if active.is_some() {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Dream Memory requires an idle Worker Session.",
            ));
        }
        let after = match &params.after_cursor {
            Some(cursor) if cursor.generation != generation => {
                return Err(TalosError::new(
                    ErrorCode::EventCursorExpired,
                    "Dream Memory cursor belongs to a cleared Session generation.",
                ))
            }
            Some(cursor) => {
                if cursor.offset < 0 {
                    return Err(conflict());
                }
                if cursor.offset > 0 {
                    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dream_formations WHERE session_id=? AND generation=? AND formation_offset=?")
                        .bind(&params.session.session_id).bind(generation as i64).bind(cursor.offset)
                        .fetch_one(&mut *tx).await.map_err(database_error)?;
                    if exists != 1 {
                        return Err(conflict());
                    }
                }
                cursor.offset
            }
            None => 0,
        };
        let row: Option<(i64,String)> = sqlx::query_as("SELECT formation_offset, formation_json FROM dream_formations WHERE session_id=? AND generation=? AND formation_offset>? ORDER BY formation_offset LIMIT 1")
            .bind(&params.session.session_id).bind(generation as i64).bind(after)
            .fetch_optional(&mut *tx).await.map_err(database_error)?;
        let (offset, formation) = match row {
            Some((offset, json)) => (offset, Some(decode(&json)?)),
            None => (after, None),
        };
        Ok(DreamMemoryResult {
            formation,
            next_cursor: DreamMemoryCursor { generation, offset },
            processed_until_seq: u64::try_from(processed_until).map_err(|error| {
                TalosError::internal_with_source("Read Worker Dream progress.", error)
            })?,
        })
    }

    /// Record observed model usage before applying Memory files or advancing their source cursor.
    pub async fn append_dream_formation(
        &self,
        session: &AgentSessionContext,
        formation: &DreamMemoryFormation,
    ) -> TalosResult<DreamMemoryCursor> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // Reserve the writer before reading its binding, as for execution append.
        sqlx::query("UPDATE sessions SET generation=generation WHERE session_id=?")
            .bind(&session.session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        if active.is_some() {
            return Err(conflict());
        }
        let offset = sqlx::query(
            "INSERT INTO dream_formations(session_id,generation,formation_json) VALUES(?,?,?)",
        )
        .bind(&session.session_id)
        .bind(generation as i64)
        .bind(encode(formation)?)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?
        .last_insert_rowid();
        tx.commit().await.map_err(database_error)?;
        Ok(DreamMemoryCursor { generation, offset })
    }

    pub async fn finish_dream_formation(
        &self,
        session: &AgentSessionContext,
        cursor: &DreamMemoryCursor,
        formation: &DreamMemoryFormation,
    ) -> TalosResult<()> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        // Reserve the writer before reading its binding, as for execution append.
        sqlx::query("UPDATE sessions SET generation=generation WHERE session_id=?")
            .bind(&session.session_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        let (generation, active) = Self::binding(&mut tx, session).await?;
        if active.is_some() || generation != cursor.generation {
            return Err(conflict());
        }
        let changed = sqlx::query("UPDATE dream_formations SET formation_json=? WHERE session_id=? AND generation=? AND formation_offset=?")
            .bind(encode(formation)?).bind(&session.session_id).bind(generation as i64).bind(cursor.offset)
            .execute(&mut *tx).await.map_err(database_error)?.rows_affected();
        if changed != 1 {
            return Err(conflict());
        }
        tx.commit().await.map_err(database_error)
    }
}
