use std::collections::HashMap;
use std::path::Path;

use chrono_tz::Tz;
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryBundleEventKind, DreamMemoryDate, DreamMemoryProvenance, MemoryScope,
};

use super::bundle::current_memory_date;
use super::source::source_from_markdown;
use super::store::{load_sources, memory_scope_lock_path, validate_scope_sources, MemoryStore};
use super::types::{MemoryRecord, MemorySearchQuery, MemorySearchRecord, ParsedMemorySource};

/// Complete source-of-truth context for replacing one raw Dream Memory file.
pub struct DreamMemorySourceReplace<'a> {
    /// Trusted Agent, user, and effective workspace scope owning the source.
    pub scope: &'a MemoryScope,
    /// Configured Dream Memory directory name.
    pub memory_dirname: &'a str,
    /// Validated date partition used to derive the source path.
    pub source_date: DreamMemoryDate,
    /// Exact content observed by the application CAS check, or `None` for a new file.
    pub expected_content: Option<&'a str>,
    /// Proposed complete canonical content.
    pub updated: &'a str,
    /// Explicit timezone used for date-partition validation.
    pub timezone: Tz,
}

impl MemoryStore {
    /// Persist one whole-file replacement and synchronize the rebuildable index.
    pub async fn replace_source(&self, edit: DreamMemorySourceReplace<'_>) -> TalosResult<()> {
        let lock_path = memory_scope_lock_path(&edit.scope.workspace_root);
        talos_fs::with_file_lock(&lock_path, || async {
            self.recover_scope_index_unlocked(edit.scope, edit.memory_dirname, edit.timezone)
                .await?;
            let source_path = format!("{}/{}.md", edit.memory_dirname, edit.source_date);
            let file_path = edit
                .scope
                .workspace_root
                .join(edit.memory_dirname)
                .join(format!("{}.md", edit.source_date));
            let current = talos_fs::read_file(&file_path).await?;
            if current.as_deref() != edit.expected_content {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "Dream Memory changed after version validation".to_string(),
                ));
            }
            let sources = self
                .validate_source_edit(&edit, &source_path, current.as_deref())
                .await?;
            mark_index_stale(&edit.scope.workspace_root).await?;
            if current.as_deref() != Some(edit.updated) {
                talos_fs::write_file(&file_path, edit.updated).await?;
            }
            self.sync_bundle_unlocked(
                edit.scope,
                edit.memory_dirname,
                &sources,
                if current.is_some() {
                    DreamMemoryBundleEventKind::Update
                } else {
                    DreamMemoryBundleEventKind::Creation
                },
                current_memory_date(edit.timezone)?,
            )
            .await?;
            self.sync_source(edit.scope, &source_path, edit.updated, edit.timezone)
                .await?;
            clear_index_stale_marker(&edit.scope.workspace_root)
        })
        .await
    }

    async fn validate_source_edit(
        &self,
        edit: &DreamMemorySourceReplace<'_>,
        source_path: &str,
        current_content: Option<&str>,
    ) -> TalosResult<Vec<ParsedMemorySource>> {
        let current = current_content
            .map(|content| {
                source_from_markdown(
                    &edit.scope.workspace_id,
                    &edit.scope.user_id,
                    source_path,
                    content,
                    edit.timezone,
                )
            })
            .transpose()?;
        let updated = source_from_markdown(
            &edit.scope.workspace_id,
            &edit.scope.user_id,
            source_path,
            edit.updated,
            edit.timezone,
        )?;
        let current_by_id = current
            .as_ref()
            .into_iter()
            .flat_map(|source| &source.records)
            .map(|parsed| (&parsed.record.memory_id, &parsed.record))
            .collect::<HashMap<_, _>>();
        for parsed in &updated.records {
            let record = &parsed.record;
            let Some(existing) = current_by_id.get(&record.memory_id) else {
                if matches!(&record.provenance, DreamMemoryProvenance::Generated { .. }) {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "new generated memory blocks must use Dream Memory formation".to_string(),
                    ));
                }
                continue;
            };
            if existing.occurred_at != record.occurred_at
                || existing.provenance != record.provenance
            {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    format!(
                        "memory provenance is immutable for {}",
                        record.memory_id.as_str()
                    ),
                ));
            }
        }
        let mut sources = load_sources(edit.scope, edit.memory_dirname, edit.timezone).await?;
        sources.retain(|source| source.source_path != source_path);
        sources.push(updated);
        validate_scope_sources(&sources)?;
        Ok(sources)
    }

    /// Search one scope while serializing against file commits and stale-index recovery.
    pub async fn search_scope(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
        query: &MemorySearchQuery,
    ) -> TalosResult<Vec<MemorySearchRecord>> {
        let lock_path = memory_scope_lock_path(&scope.workspace_root);
        talos_fs::with_file_lock(&lock_path, || async {
            self.recover_scope_index_unlocked(scope, memory_dirname, timezone)
                .await?;
            self.search(scope, query).await
        })
        .await
    }

    /// Get one entry while serializing against file commits and stale-index recovery.
    pub async fn get_scope(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
        memory_id: &str,
    ) -> TalosResult<Option<MemoryRecord>> {
        let lock_path = memory_scope_lock_path(&scope.workspace_root);
        talos_fs::with_file_lock(&lock_path, || async {
            self.recover_scope_index_unlocked(scope, memory_dirname, timezone)
                .await?;
            self.get(scope, memory_id).await
        })
        .await
    }

    pub(super) async fn recover_scope_index_unlocked(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
    ) -> TalosResult<()> {
        if index_stale_marker_exists(&scope.workspace_root).await? {
            self.rebuild_scope_unlocked(scope, memory_dirname, timezone)
                .await?;
        }
        Ok(())
    }
}

fn index_stale_marker_path(workspace_root: &Path) -> std::path::PathBuf {
    workspace_root.join(".dream-memory-index-stale")
}

async fn index_stale_marker_exists(workspace_root: &Path) -> TalosResult<bool> {
    Ok(
        talos_fs::get_file_metadata(&index_stale_marker_path(workspace_root))
            .await?
            .is_some(),
    )
}

pub(super) async fn mark_index_stale(workspace_root: &Path) -> TalosResult<()> {
    talos_fs::write_file(&index_stale_marker_path(workspace_root), "stale\n").await
}

pub(super) fn clear_index_stale_marker(workspace_root: &Path) -> TalosResult<()> {
    talos_fs::remove_file(&index_stale_marker_path(workspace_root))
}
