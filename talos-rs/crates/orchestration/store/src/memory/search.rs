use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Row, Sqlite};
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryDate, DreamMemoryProvenance, DreamMemorySourceRange, MemoryId, MemoryScope,
    MemorySessionId,
};

use super::store::{storage_error, MemoryStore};
use super::types::{
    memory_kind_as_str, parse_memory_importance, parse_memory_kind, MemoryIndexScope, MemoryRecord,
    MemorySearchQuery, MemorySearchRecord,
};

impl MemoryStore {
    /// Search one user scope with deterministic ranking and bounded results.
    pub(super) async fn search(
        &self,
        scope: &MemoryScope,
        query: &MemorySearchQuery,
    ) -> TalosResult<Vec<MemorySearchRecord>> {
        let index_scope = MemoryIndexScope::from_scope(scope)?;
        validate_search_query(query)?;
        let text = query.text.trim();
        let rows = if text.chars().count() < 3 {
            self.search_short(&index_scope, query).await?
        } else {
            self.search_fts(&index_scope, query).await?
        };
        rows.into_iter()
            .enumerate()
            .map(|(index, row)| {
                let memory = record_from_row(&row)?;
                Ok(MemorySearchRecord {
                    preview: build_preview(&memory.body, text),
                    memory,
                    rank: u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX),
                })
            })
            .collect()
    }

    /// Read one stable memory ID from a user scope.
    pub(super) async fn get(
        &self,
        scope: &MemoryScope,
        memory_id: &str,
    ) -> TalosResult<Option<MemoryRecord>> {
        let index_scope = MemoryIndexScope::from_scope(scope)?;
        if memory_id.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "memory_id is required".to_string(),
            ));
        }
        sqlx::query(&format!(
            "SELECT {} FROM memory_entries WHERE workspace_id = ? AND user_id = ? AND workspace_root = ? AND memory_id = ?",
            record_columns()
        ))
        .bind(index_scope.workspace_id())
        .bind(index_scope.user_id())
        .bind(index_scope.workspace_root())
        .bind(memory_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage_error("get memory"))?
        .map(|row| record_from_row(&row))
        .transpose()
    }

    async fn search_fts(
        &self,
        scope: &MemoryIndexScope,
        query: &MemorySearchQuery,
    ) -> TalosResult<Vec<SqliteRow>> {
        let mut sql = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {} FROM memory_entries_fts JOIN memory_entries e ON e.row_id = memory_entries_fts.rowid WHERE memory_entries_fts MATCH ",
            record_columns_with_alias("e")
        ));
        sql.push_bind(fts_literal(query.text.trim()));
        push_search_filters(&mut sql, scope, query);
        sql.push(
            " ORDER BY bm25(memory_entries_fts, 2.0, 1.0) * CASE e.importance WHEN 'high' THEN 1.2 WHEN 'low' THEN 0.8 ELSE 1.0 END ASC, e.occurred_at DESC, e.memory_id ASC LIMIT ",
        );
        sql.push_bind(i64::from(query.limit));
        sql.build()
            .fetch_all(&self.pool)
            .await
            .map_err(storage_error("search memory FTS"))
    }

    async fn search_short(
        &self,
        scope: &MemoryIndexScope,
        query: &MemorySearchQuery,
    ) -> TalosResult<Vec<SqliteRow>> {
        let mut sql = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {} FROM memory_entries e WHERE (instr(lower(e.title), lower(",
            record_columns_with_alias("e")
        ));
        sql.push_bind(query.text.trim());
        sql.push(")) > 0 OR instr(lower(e.body), lower(");
        sql.push_bind(query.text.trim());
        sql.push(")) > 0)");
        push_search_filters(&mut sql, scope, query);
        sql.push(" ORDER BY CASE WHEN instr(lower(e.title), lower(");
        sql.push_bind(query.text.trim());
        sql.push(")) > 0 THEN 0 ELSE 1 END ASC, CASE WHEN instr(lower(e.title), lower(");
        sql.push_bind(query.text.trim());
        sql.push(")) > 0 THEN instr(lower(e.title), lower(");
        sql.push_bind(query.text.trim());
        sql.push(")) ELSE instr(lower(e.body), lower(");
        sql.push_bind(query.text.trim());
        sql.push(")) END ASC, CASE e.importance WHEN 'high' THEN 0 WHEN 'normal' THEN 1 ELSE 2 END ASC, e.occurred_at DESC, e.memory_id ASC LIMIT ");
        sql.push_bind(i64::from(query.limit));
        sql.build()
            .fetch_all(&self.pool)
            .await
            .map_err(storage_error("search short memory query"))
    }
}

fn push_search_filters<'a>(
    sql: &mut QueryBuilder<'a, Sqlite>,
    scope: &'a MemoryIndexScope,
    query: &MemorySearchQuery,
) {
    sql.push(" AND e.workspace_id = ")
        .push_bind(scope.workspace_id())
        .push(" AND e.user_id = ")
        .push_bind(scope.user_id())
        .push(" AND e.workspace_root = ")
        .push_bind(scope.workspace_root());
    if let Some(kinds) = &query.kinds {
        sql.push(" AND e.kind IN (");
        let mut separated = sql.separated(", ");
        for kind in kinds {
            separated.push_bind(memory_kind_as_str(*kind));
        }
        separated.push_unseparated(")");
    }
    if let Some(since_at) = query.since_at {
        sql.push(" AND e.occurred_at >= ").push_bind(since_at);
    }
    if let Some(until_at) = query.until_at {
        sql.push(" AND e.occurred_at <= ").push_bind(until_at);
    }
}

fn validate_search_query(query: &MemorySearchQuery) -> TalosResult<()> {
    let text = query.text.trim();
    if text.is_empty()
        || text.chars().count() > 512
        || text.chars().any(char::is_control)
        || !(1..=20).contains(&query.limit)
        || query.kinds.as_ref().is_some_and(Vec::is_empty)
        || query
            .since_at
            .zip(query.until_at)
            .is_some_and(|(since, until)| since > until)
    {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "invalid memory search request".to_string(),
        ));
    }
    Ok(())
}

fn fts_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn build_preview(body: &str, query: &str) -> String {
    let body_chars = body.chars().collect::<Vec<_>>();
    let folded_body = body.to_lowercase();
    let folded_query = query.to_lowercase();
    let match_start = folded_body
        .find(&folded_query)
        .map(|byte_index| folded_body[..byte_index].chars().count());
    let start = match_start.unwrap_or(0).saturating_sub(80);
    body_chars
        .into_iter()
        .skip(start)
        .take(300)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn record_columns() -> &'static str {
    "memory_id, title, body, kind, importance, occurred_at, source_date, session_id, source_from_seq, source_until_seq"
}

fn record_columns_with_alias(alias: &str) -> String {
    record_columns()
        .split(", ")
        .map(|column| format!("{alias}.{column}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn record_from_row(row: &SqliteRow) -> TalosResult<MemoryRecord> {
    let kind = row
        .try_get::<String, _>("kind")
        .map_err(storage_error("decode memory kind"))?;
    let importance = row
        .try_get::<String, _>("importance")
        .map_err(storage_error("decode memory importance"))?;
    let memory_id = row
        .try_get::<String, _>("memory_id")
        .map_err(storage_error("decode memory_id"))?;
    let source_date = row
        .try_get::<String, _>("source_date")
        .map_err(storage_error("decode memory source date"))?;
    let session_id = row
        .try_get::<Option<String>, _>("session_id")
        .map_err(storage_error("decode memory session"))?;
    let source_from_seq = optional_u64(row, "source_from_seq")?;
    let source_until_seq = optional_u64(row, "source_until_seq")?;
    Ok(MemoryRecord {
        memory_id: MemoryId::parse(memory_id).map_err(invalid_stored_memory)?,
        title: row
            .try_get("title")
            .map_err(storage_error("decode memory title"))?,
        body: row
            .try_get("body")
            .map_err(storage_error("decode memory body"))?,
        kind: parse_memory_kind(&kind)
            .ok_or_else(|| TalosError::internal(format!("invalid stored memory kind: {kind}")))?,
        importance: parse_memory_importance(&importance).ok_or_else(|| {
            TalosError::internal(format!("invalid stored memory importance: {importance}"))
        })?,
        occurred_at: row
            .try_get("occurred_at")
            .map_err(storage_error("decode memory occurred_at"))?,
        source_date: DreamMemoryDate::parse(&source_date).map_err(invalid_stored_memory)?,
        provenance: decode_provenance(session_id, source_from_seq, source_until_seq)?,
    })
}

fn decode_provenance(
    session_id: Option<String>,
    source_from_seq: Option<u64>,
    source_until_seq: Option<u64>,
) -> TalosResult<DreamMemoryProvenance> {
    match (session_id, source_from_seq, source_until_seq) {
        (None, None, None) => Ok(DreamMemoryProvenance::UserAuthored),
        (Some(session_id), Some(from_seq), Some(until_seq)) => {
            Ok(DreamMemoryProvenance::Generated {
                session_id: MemorySessionId::parse(session_id).map_err(invalid_stored_memory)?,
                source_range: DreamMemorySourceRange::new(from_seq, until_seq)
                    .map_err(invalid_stored_memory)?,
            })
        }
        _ => Err(TalosError::internal(
            "stored Dream Memory provenance is incomplete".to_string(),
        )),
    }
}

fn invalid_stored_memory(error: talos_memory::DreamMemoryError) -> TalosError {
    TalosError::internal(format!("invalid stored Dream Memory: {error}"))
}

fn optional_u64(row: &SqliteRow, column: &str) -> TalosResult<Option<u64>> {
    row.try_get::<Option<i64>, _>(column)
        .map_err(storage_error("decode memory sequence"))?
        .map(|value| {
            u64::try_from(value).map_err(|_| {
                TalosError::internal(format!("invalid negative memory sequence: {value}"))
            })
        })
        .transpose()
}
