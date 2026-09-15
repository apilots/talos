use std::path::{Path, PathBuf};

use talos_error::{TalosError, TalosResult};
use talos_memory::{MemoryAgentId, UserWorkspaceMemoryDocument, UserWorkspaceMemoryProducer};

/// Prepared replacement with a durable, immutable source for interrupted upgrades.
pub(super) struct FileRepair {
    pub path: PathBuf,
    pub backup: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

impl FileRepair {
    pub fn apply(&self) -> TalosResult<()> {
        let current = std::fs::read(&self.path).map_err(super::source_error)?;
        if current == self.after {
            return Ok(());
        }
        if current != self.before {
            return Err(super::invalid("repair source changed after validation")
                .context(format!("repair file {}", self.path.display())));
        }
        if self.backup.exists() {
            if std::fs::read(&self.backup).map_err(super::source_error)? != self.before {
                return Err(super::invalid("repair backup differs from source")
                    .context(format!("repair backup {}", self.backup.display())));
            }
        } else {
            talos_fs::write_bytes_with(
                &self.backup,
                &self.before,
                talos_fs::WriteOptions::create_new(),
            )?;
        }
        talos_fs::write_bytes(&self.path, &self.after)?;
        tracing::info!(path = %self.path.display(), "upgrade file repaired; original retained");
        Ok(())
    }
}

/// Reuse the original after a partial filesystem phase; the DB transaction is
/// published only after every replacement is durable.
pub(super) fn source(path: &Path, backup: &Path) -> TalosResult<Vec<u8>> {
    std::fs::read(if backup.exists() { backup } else { path }).map_err(|error| {
        super::source_error(error).context(format!("read repair source {}", path.display()))
    })
}

pub(super) fn memory(content: &str, workspace_id: &str, generated_at: i64) -> TalosResult<String> {
    if UserWorkspaceMemoryDocument::parse(content).is_ok() {
        return Ok(content.to_owned());
    }
    // Damaged managed documents need explicit repair, not a second managed envelope.
    if content.contains("talos:user-workspace-memory")
        || content.trim_start().starts_with("---\nokf:")
    {
        return Err(super::invalid(
            "invalid structured MEMORY.md; refusing to wrap it",
        ));
    }
    let producer = UserWorkspaceMemoryProducer::new(
        MemoryAgentId::parse(workspace_id).map_err(super::source_error)?,
        generated_at,
    );
    let template = UserWorkspaceMemoryDocument::template(producer)
        .and_then(|document| document.render())
        .map_err(super::source_error)?;
    let output = format!("{}\n\n{content}", template.trim_end());
    UserWorkspaceMemoryDocument::parse(&output).map_err(|error| {
        TalosError::internal_with_source("Memory upgrade validation failed.", error)
    })?;
    Ok(output)
}
