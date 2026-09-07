//! One-time adapter that converts legacy date-partitioned memory Markdown to Dream Memory OKF.
//!
//! This module is intentionally outside every normal read/write path and is called only by the
//! versioned Product Upgrade task.

mod parser;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use talos_config::TalosTimezone;
use talos_error::{TalosError, TalosResult};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryEntry,
    MemoryAgentId, MemoryTimezone, MemoryUserId,
};
use talos_store::memory::memory_scope_lock_path;

use self::parser::{legacy_partition_start, parse_legacy_entries};
const DREAM_MEMORY_MARKER: &str = "talos:dream-memory";

struct SourceRevision {
    path: PathBuf,
    version: String,
}

#[derive(Debug)]
pub(super) struct LegacyDreamMemoryConversion {
    backup_dir: PathBuf,
}

impl LegacyDreamMemoryConversion {
    pub(super) async fn finish(self) -> TalosResult<()> {
        talos_fs::remove_directory(&self.backup_dir).await
    }
}

/// Converts one user scope through a staged directory switch when legacy files are present.
///
pub(super) async fn convert_legacy_scope(
    workspace_id: &str,
    user_id: &str,
    workspace_root: &Path,
    memory_dirname: &str,
    timezone: TalosTimezone,
) -> TalosResult<Option<LegacyDreamMemoryConversion>> {
    let memory_dir = workspace_root.join(memory_dirname);
    let staging_dir = workspace_root.join(format!(".{memory_dirname}.dream-memory-okf-staging"));
    let backup_dir = workspace_root.join(format!(".{memory_dirname}.legacy-backup"));
    recover_interrupted_switch(
        workspace_root,
        memory_dirname,
        &memory_dir,
        &staging_dir,
        &backup_dir,
    )
    .await?;
    let Some(entries) = talos_fs::read_directory(&memory_dir)? else {
        return Ok(None);
    };
    let backup_exists = talos_fs::get_file_metadata(&backup_dir).await?.is_some();
    let mut source_revisions = Vec::new();
    let mut staged_files = Vec::new();
    let mut contains_legacy = false;

    for entry in entries {
        if !entry.metadata.is_file || entry.metadata.is_symlink {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "Dream Memory conversion only accepts regular files: {}",
                    entry.path.display()
                ),
            ));
        }
        let name = entry
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| TalosError::internal("invalid Dream Memory filename"))?;
        let content = talos_fs::read_file(&entry.path)
            .await?
            .ok_or_else(|| TalosError::internal("Dream Memory source disappeared"))?;
        source_revisions.push(SourceRevision {
            path: entry.path.clone(),
            version: workspace_revision(&content),
        });
        let staged = if let Some(date) = memory_file_date(name) {
            let context = document_context(workspace_id, user_id, date, timezone)?;
            match DreamMemoryDocument::parse(context.clone(), &content) {
                Ok(document) => document.render().map_err(map_domain_error)?,
                Err(_) if content.contains(DREAM_MEMORY_MARKER) => {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        format!(
                            "invalid canonical Dream Memory file: {}",
                            entry.path.display()
                        ),
                    ));
                }
                Err(_) => {
                    contains_legacy = true;
                    let memories = parse_legacy_entries(user_id, date, &content, timezone)?;
                    let generated_at =
                        match memories.iter().map(DreamMemoryEntry::occurred_at).min() {
                            Some(generated_at) => generated_at,
                            None => legacy_partition_start(date, timezone)?,
                        };
                    let mut document = DreamMemoryDocument::new(context, generated_at)
                        .map_err(map_domain_error)?;
                    for memory in memories {
                        document.create_entry(memory).map_err(map_domain_error)?;
                    }
                    document.render().map_err(map_domain_error)?
                }
            }
        } else {
            content
        };
        staged_files.push((name.to_string(), staged));
    }

    if !contains_legacy {
        return if backup_exists {
            Ok(Some(LegacyDreamMemoryConversion { backup_dir }))
        } else {
            Ok(None)
        };
    }
    if backup_exists {
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            format!(
                "legacy Dream Memory backup requires recovery: {}",
                backup_dir.display()
            ),
        ));
    }

    talos_fs::remove_directory(&staging_dir).await?;
    talos_fs::create_directory(&staging_dir, true)?;
    for (name, content) in staged_files {
        talos_fs::write_file(&staging_dir.join(name), &content).await?;
    }

    let lock_path = memory_scope_lock_path(workspace_root);
    talos_fs::with_file_lock(&lock_path, || async {
        verify_source_revisions(&memory_dir, &source_revisions).await?;
        switch_memory_directories(&memory_dir, &staging_dir, &backup_dir)
    })
    .await?;
    Ok(Some(LegacyDreamMemoryConversion { backup_dir }))
}

fn workspace_revision(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn switch_memory_directories(
    memory_dir: &Path,
    staging_dir: &Path,
    backup_dir: &Path,
) -> TalosResult<()> {
    talos_fs::rename(memory_dir, backup_dir)?;
    if let Err(error) = talos_fs::rename(staging_dir, memory_dir) {
        talos_fs::rename(backup_dir, memory_dir).map_err(|restore_error| {
            talos_logging::report_error(&error);
            restore_error.context("restore legacy directory after failed Dream Memory switch")
        })?;
        return Err(error);
    }
    Ok(())
}

async fn recover_interrupted_switch(
    workspace_root: &Path,
    _memory_dirname: &str,
    memory_dir: &Path,
    staging_dir: &Path,
    backup_dir: &Path,
) -> TalosResult<()> {
    let lock_path = memory_scope_lock_path(workspace_root);
    talos_fs::with_file_lock(&lock_path, || async {
        let memory_exists = talos_fs::get_file_metadata(memory_dir).await?.is_some();
        let backup_exists = talos_fs::get_file_metadata(backup_dir).await?.is_some();
        let staging_exists = talos_fs::get_file_metadata(staging_dir).await?.is_some();
        if !memory_exists && backup_exists {
            talos_fs::rename(backup_dir, memory_dir)?;
            talos_fs::remove_directory(staging_dir).await?;
            return Ok(());
        }
        if !memory_exists && staging_exists {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Dream Memory staging exists without a source or backup directory".to_string(),
            ));
        }
        Ok(())
    })
    .await
}

async fn verify_source_revisions(memory_dir: &Path, sources: &[SourceRevision]) -> TalosResult<()> {
    let current_paths = talos_fs::read_directory(memory_dir)?
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.path)
        .collect::<BTreeSet<_>>();
    let expected_paths = sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    if current_paths != expected_paths {
        return Err(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "legacy Dream Memory file set changed during conversion".to_string(),
        ));
    }
    for source in sources {
        let content = talos_fs::read_file(&source.path).await?;
        if content
            .as_deref()
            .is_none_or(|content| workspace_revision(content) != source.version)
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "legacy Dream Memory changed during conversion".to_string(),
            ));
        }
    }
    Ok(())
}

fn memory_file_date(name: &str) -> Option<&str> {
    let date = name.strip_suffix(".md")?;
    (date.len() == 10).then_some(date)
}

fn document_context(
    workspace_id: &str,
    user_id: &str,
    date: &str,
    timezone: TalosTimezone,
) -> TalosResult<DreamMemoryDocumentContext> {
    Ok(DreamMemoryDocumentContext::new(
        MemoryAgentId::parse(workspace_id.to_string()).map_err(map_domain_error)?,
        MemoryUserId::parse(user_id.to_string()).map_err(map_domain_error)?,
        DreamMemoryDate::parse(date).map_err(map_domain_error)?,
        MemoryTimezone::new(timezone),
    ))
}

fn map_domain_error(error: talos_memory::DreamMemoryError) -> TalosError {
    TalosError::new(talos_error::ErrorCode::InvalidRequest, error.to_string())
}

#[cfg(test)]
#[path = "converter_tests.rs"]
mod tests;
