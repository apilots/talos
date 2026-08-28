use chrono::{Datelike, Utc};
use chrono_tz::Tz;
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryBundle, DreamMemoryBundleConcept, DreamMemoryBundleEventKind,
    DreamMemoryConceptRevision, DreamMemoryDate, MemoryScope,
};

use super::source::map_dream_memory_error;
use super::store::memory_scope_lock_path;
use super::types::ParsedMemorySource;
use super::MemoryStore;

const DREAM_MEMORY_INDEX_FILE: &str = "index.md";
const DREAM_MEMORY_LOG_FILE: &str = "log.md";

impl MemoryStore {
    /// Lists canonical date partitions from the OKF navigation index.
    pub async fn list_scope_dates(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        timezone: Tz,
    ) -> TalosResult<Vec<DreamMemoryDate>> {
        let lock_path = memory_scope_lock_path(&scope.workspace_root);
        talos_fs::with_file_lock(&lock_path, || async {
            self.recover_scope_index_unlocked(scope, memory_dirname, timezone)
                .await?;
            let index_path = scope
                .workspace_root
                .join(memory_dirname)
                .join(DREAM_MEMORY_INDEX_FILE);
            let content = talos_fs::read_file(&index_path).await?;
            if let Some(content) = content.as_deref() {
                if let Ok(dates) = DreamMemoryBundle::parse_index_dates(content) {
                    return Ok(dates);
                }
            }
            self.rebuild_scope_unlocked(scope, memory_dirname, timezone)
                .await?;
            let content = talos_fs::read_file(&index_path)
                .await?
                .ok_or_else(|| TalosError::internal("Dream Memory index was not rebuilt"))?;
            DreamMemoryBundle::parse_index_dates(&content).map_err(map_dream_memory_error)
        })
        .await
    }

    pub(super) async fn sync_bundle_unlocked(
        &self,
        scope: &MemoryScope,
        memory_dirname: &str,
        sources: &[ParsedMemorySource],
        event_kind: DreamMemoryBundleEventKind,
        logged_on: DreamMemoryDate,
    ) -> TalosResult<()> {
        let memory_dir = scope.workspace_root.join(memory_dirname);
        talos_fs::create_directory(&memory_dir, true)?;
        let log_path = memory_dir.join(DREAM_MEMORY_LOG_FILE);
        let current_log = talos_fs::read_file(&log_path).await?;
        let concepts = sources
            .iter()
            .map(|source| {
                DreamMemoryBundleConcept::new(
                    source.source_date,
                    DreamMemoryConceptRevision::parse(source.fingerprint.clone())
                        .map_err(map_dream_memory_error)?,
                    source
                        .records
                        .iter()
                        .map(|record| record.record.title.clone())
                        .collect(),
                )
                .map_err(map_dream_memory_error)
            })
            .collect::<TalosResult<Vec<_>>>()?;
        let bundle =
            DreamMemoryBundle::reconcile(&concepts, current_log.as_deref(), event_kind, logged_on)
                .map_err(map_dream_memory_error)?;
        write_if_changed(
            &memory_dir.join(DREAM_MEMORY_INDEX_FILE),
            &bundle.render_index(),
        )
        .await?;
        if let Some(log) = bundle.render_log() {
            write_if_changed(&log_path, &log).await?;
        } else {
            talos_fs::remove_file(&log_path)?;
        }
        Ok(())
    }
}

pub(super) fn current_memory_date(timezone: Tz) -> TalosResult<DreamMemoryDate> {
    let now = Utc::now().with_timezone(&timezone);
    DreamMemoryDate::parse(&format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        now.month(),
        now.day()
    ))
    .map_err(map_dream_memory_error)
}

async fn write_if_changed(path: &std::path::Path, content: &str) -> TalosResult<()> {
    if talos_fs::read_file(path).await?.as_deref() != Some(content) {
        talos_fs::write_file(path, content).await?;
    }
    Ok(())
}
