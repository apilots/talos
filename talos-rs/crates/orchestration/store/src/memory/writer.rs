use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryAppendResult, DreamMemoryBundleEventKind, DreamMemoryDate, DreamMemoryDayRevision,
    DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryWriteRequest, MemoryAgentId,
    MemoryScope, MemoryTimezone, MemoryUserId,
};

use super::bundle::current_memory_date;
use super::consistency::{clear_index_stale_marker, mark_index_stale};
use super::source::{map_dream_memory_error, source_from_markdown};
use super::store::{load_sources, validate_scope_sources};
use super::{memory_scope_lock_path, MemoryStore};

impl MemoryStore {
    /// Atomically append one idempotent formation batch and synchronize its derived index.
    pub async fn write_batch(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
        request: &DreamMemoryWriteRequest,
    ) -> TalosResult<()> {
        let batch = request.batch();
        let timestamp = Utc
            .timestamp_opt(batch.occurred_at, 0)
            .single()
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    "invalid Dream Memory occurred_at".to_string(),
                )
            })?;
        let source_date = timestamp
            .with_timezone(&timezone)
            .format("%Y-%m-%d")
            .to_string();
        if request.date().to_string() != source_date {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "Dream Memory write date does not match occurred_at".to_string(),
            ));
        }
        let source_path = format!("{memory_dirname}/{source_date}.md");
        let file_path = scope.workspace_root.join(&source_path);
        let lock_path = memory_scope_lock_path(&scope.workspace_root);

        talos_fs::with_file_lock(&lock_path, || async {
            self.recover_scope_index_unlocked(scope, memory_dirname, timezone)
                .await?;
            let current = talos_fs::read_file(&file_path).await?;
            if &DreamMemoryDayRevision::from_content(current.as_deref()) != request.base_revision()
            {
                return Err(TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    "Dream Memory day revision mismatch".to_string(),
                ));
            }
            let context = DreamMemoryDocumentContext::new(
                MemoryAgentId::parse(scope.agent_id.clone()).map_err(map_dream_memory_error)?,
                MemoryUserId::parse(scope.user_id.clone()).map_err(map_dream_memory_error)?,
                DreamMemoryDate::parse(&source_date).map_err(map_dream_memory_error)?,
                MemoryTimezone::new(timezone),
            );
            let mut document = match current.as_deref() {
                Some(markdown) => DreamMemoryDocument::parse(context, markdown),
                None => DreamMemoryDocument::new(context, batch.occurred_at),
            }
            .map_err(map_dream_memory_error)?;
            match document
                .append_batch(batch)
                .map_err(map_dream_memory_error)?
            {
                DreamMemoryAppendResult::AlreadyApplied => {
                    if let Some(markdown) = current.as_deref() {
                        self.sync_source(scope, &source_path, markdown, timezone)
                            .await?;
                    }
                    Ok(())
                }
                DreamMemoryAppendResult::Appended => {
                    let updated = document.render().map_err(map_dream_memory_error)?;
                    let updated_source = source_from_markdown(
                        &scope.agent_id,
                        &scope.user_id,
                        &source_path,
                        &updated,
                        timezone,
                    )?;
                    let mut sources = load_sources(scope, memory_dirname, timezone).await?;
                    sources.retain(|source| source.source_path != source_path);
                    sources.push(updated_source);
                    validate_scope_sources(&sources)?;

                    mark_index_stale(&scope.workspace_root).await?;
                    talos_fs::write_file(&file_path, &updated).await?;
                    self.sync_bundle_unlocked(
                        scope,
                        memory_dirname,
                        &sources,
                        if current.is_some() {
                            DreamMemoryBundleEventKind::Update
                        } else {
                            DreamMemoryBundleEventKind::Creation
                        },
                        current_memory_date(timezone)?,
                    )
                    .await?;
                    self.sync_source(scope, &source_path, &updated, timezone)
                        .await?;
                    clear_index_stale_marker(&scope.workspace_root)
                }
            }
        })
        .await
    }
}
