use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use talos_error::{TalosError, TalosResult};
use talos_memory::MemoryWriteBatch;

use super::parser::{normalize_text, parse_source, sha256_hex};
use super::types::{memory_importance_as_str, memory_kind_as_str, MemoryRecord};
use super::MemoryStore;

#[derive(Serialize)]
struct MemoryIdKey<'a> {
    schema_version: u8,
    user_id: &'a str,
    session_id: &'a str,
    source_from_seq: u64,
    source_until_seq: u64,
    candidate_ordinal: usize,
}

impl MemoryStore {
    /// Atomically append one idempotent formation batch and synchronize its index source.
    pub async fn write_batch(
        &self,
        user_id: &str,
        workspace_root: &std::path::Path,
        memory_dirname: &str,
        timezone: Tz,
        batch: &MemoryWriteBatch,
    ) -> TalosResult<()> {
        validate_batch(user_id, batch)?;
        let timestamp = Utc
            .timestamp_opt(batch.occurred_at, 0)
            .single()
            .ok_or_else(|| TalosError::ValidationError("invalid memory occurred_at".to_string()))?;
        let source_date = timestamp
            .with_timezone(&timezone)
            .format("%Y-%m-%d")
            .to_string();
        let source_path = format!("{memory_dirname}/{source_date}.md");
        let file_path = workspace_root.join(&source_path);

        talos_fs::with_file_lock(&file_path, || async {
            let current = talos_fs::read_file(&file_path).await?.unwrap_or_default();
            let parsed = parse_source(user_id, &source_path, &current, timezone)?;
            if parsed.records.iter().any(|parsed| {
                parsed.record.session_id.as_deref() == Some(batch.session_id.as_str())
                    && parsed.record.source_from_seq == Some(batch.source_from_seq)
                    && parsed.record.source_until_seq == Some(batch.source_until_seq)
            }) {
                return self
                    .sync_source(user_id, &source_path, &current, timezone)
                    .await;
            }
            if batch.candidates.is_empty() {
                return Ok(());
            }
            let expected = expected_records(user_id, &source_date, batch)?;
            let expected_ids = expected
                .iter()
                .map(|record| record.memory_id.clone())
                .collect::<Vec<_>>();
            if parsed.records.iter().any(|parsed| {
                parsed.record.session_id.as_deref() == Some(batch.session_id.as_str())
                    && parsed.record.source_from_seq.is_some_and(|from| {
                        from <= batch.source_until_seq
                            && parsed
                                .record
                                .source_until_seq
                                .is_some_and(|until| until >= batch.source_from_seq)
                    })
                    && !(parsed.record.source_from_seq == Some(batch.source_from_seq)
                        && parsed.record.source_until_seq == Some(batch.source_until_seq))
            }) {
                return Err(TalosError::Conflict(
                    "memory write batch overlaps an existing session range".to_string(),
                ));
            }
            self.validate_batch_append(
                user_id,
                &source_path,
                &batch.session_id,
                batch.source_from_seq,
                batch.source_until_seq,
                &expected_ids,
            )
            .await?;

            let mut updated = current;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            for record in &expected {
                updated.push_str(&render_record(record, timezone)?);
            }
            parse_source(user_id, &source_path, &updated, timezone)?;
            talos_fs::write_file(&file_path, &updated).await?;
            self.sync_source(user_id, &source_path, &updated, timezone)
                .await
        })
        .await
    }
}

fn validate_batch(user_id: &str, batch: &MemoryWriteBatch) -> TalosResult<()> {
    if user_id.trim().is_empty()
        || batch.session_id.trim().is_empty()
        || batch.source_from_seq > batch.source_until_seq
        || batch.candidates.len() > 8
        || batch.candidates.iter().any(|candidate| {
            let title = normalize_text(&candidate.title);
            let body = normalize_text(&candidate.body);
            title.is_empty()
                || body.is_empty()
                || title.chars().count() > 160
                || body.chars().count() > 1_000
        })
    {
        return Err(TalosError::ValidationError(
            "invalid memory write batch".to_string(),
        ));
    }
    Ok(())
}

fn expected_records(
    user_id: &str,
    source_date: &str,
    batch: &MemoryWriteBatch,
) -> TalosResult<Vec<MemoryRecord>> {
    batch
        .candidates
        .iter()
        .enumerate()
        .map(|(ordinal, candidate)| {
            let key = MemoryIdKey {
                schema_version: 1,
                user_id,
                session_id: &batch.session_id,
                source_from_seq: batch.source_from_seq,
                source_until_seq: batch.source_until_seq,
                candidate_ordinal: ordinal,
            };
            let bytes = serde_json::to_vec(&key).map_err(|error| {
                TalosError::SerializationError(format!("serialize memory id: {error}"))
            })?;
            Ok(MemoryRecord {
                memory_id: format!("mem_{}", &sha256_hex(&bytes)[..32]),
                title: normalize_text(&candidate.title),
                body: normalize_text(&candidate.body),
                kind: candidate.kind,
                importance: candidate.importance,
                occurred_at: batch.occurred_at,
                source_date: source_date.to_string(),
                session_id: Some(batch.session_id.clone()),
                source_from_seq: Some(batch.source_from_seq),
                source_until_seq: Some(batch.source_until_seq),
            })
        })
        .collect()
}

fn render_record(record: &MemoryRecord, timezone: Tz) -> TalosResult<String> {
    let timestamp = Utc
        .timestamp_opt(record.occurred_at, 0)
        .single()
        .ok_or_else(|| TalosError::ValidationError("invalid memory occurred_at".to_string()))?;
    Ok(format!(
        "## [{}] {}\nmemory_id: {}\noccurred_at: {}\nsession: {}\nkind: {}\nimportance: {}\nsource_from_seq: {}\nsource_until_seq: {}\n\n{}\n\n",
        timestamp.with_timezone(&timezone).format("%H:%M"),
        record.title,
        record.memory_id,
        record.occurred_at,
        record.session_id.as_deref().unwrap_or_default(),
        memory_kind_as_str(record.kind),
        memory_importance_as_str(record.importance),
        record.source_from_seq.unwrap_or_default(),
        record.source_until_seq.unwrap_or_default(),
        record.body,
    ))
}
