use super::preparation::StagingDirectory;
use super::*;
use talos_types::MarketplaceSource;

impl PluginManager {
    /// Materialization uses an isolated cache with no installation file lock held.
    /// Publication rechecks the source and cache revision before replacing cached content.
    pub(super) fn refresh_source_staged(
        &self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        let (source, previous) = self
            .state
            .with_exclusive_lock("plugin.source.inspect", || {
                let source = self.source_by_id(&request.source_id)?;
                let previous = self.state.load_source_scan_cache(&source.id)?;
                Ok((source, previous))
            })?;
        let directory = StagingDirectory::new(&self.state)?;
        let staging = self.state.staging_store(directory.path().join("source"));
        let scan = crate::scanner::scan_source(&staging, &source)?;
        self.publish_source_scan(source, previous, &directory, scan)
    }

    pub(super) fn publish_source_scan(
        &self,
        mut source: MarketplaceSource,
        previous: crate::state::SourceScanCacheFile,
        directory: &StagingDirectory,
        scan: crate::scanner::SourceScanResult,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        let staging = self.state.staging_store(directory.path().join("source"));
        source.last_checked_commit = scan.source_commit.clone();
        source.last_checked_at_ms = Some(scanned_at_ms());
        source.updated_at_ms = source.last_checked_at_ms.unwrap_or_default();
        let plugins = plugins_with_source_commit(scan.plugins, source.last_checked_commit.clone());
        let warnings = scan.warnings;
        // Publish source bytes and their scan index together. An interrupted rename may
        // lose a disposable cache, but can never label new bytes with an old commit.
        staging.save_source_scan_cache(
            &source.id,
            &crate::state::SourceScanCacheFile {
                version: 1,
                source_id: source.id.clone(),
                scanned_at_ms: source.updated_at_ms,
                source_commit: source.last_checked_commit.clone(),
                plugins: plugins.clone(),
                warnings: warnings.clone(),
            },
        )?;
        self.state.with_exclusive_lock("plugin.source.publish", || {
            let current_source = self.source_by_id(&source.id)?;
            let current = self.state.load_source_scan_cache(&source.id)?;
            if current_source.location != source.location
                || current_source.sparse_paths != source.sparse_paths
                || current.scanned_at_ms != previous.scanned_at_ms
                || current.source_commit != previous.source_commit
                || current.plugins != previous.plugins
            {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "The plugin source changed while it was being refreshed.",
                ));
            }
            let target = self.state.marketplace_cache_dir(&source.id);
            let backup = directory.path().join("previous");
            talos_fs::create_directory(
                target
                    .parent()
                    .ok_or_else(|| TalosError::internal("The plugin cache root is invalid."))?,
                true,
            )?;
            if target.exists() {
                rename(&target, &backup)?;
            }
            if let Err(error) = rename(&staging.marketplace_cache_dir(&source.id), &target) {
                if backup.exists() {
                    rename(&backup, &target)?;
                }
                return Err(error);
            }
            let mut file = self.state.load_sources()?;
            if let Some(record) = file
                .sources
                .iter_mut()
                .find(|record| record.id == source.id)
            {
                *record = source.clone();
                self.state.save_sources(&file)?;
            }
            Ok(PluginSourceRefreshResponse {
                source,
                plugins,
                warnings,
            })
        })
    }

    pub(super) fn source_by_id(
        &self,
        id: &talos_types::MarketplaceSourceId,
    ) -> TalosResult<MarketplaceSource> {
        self.list_sources(PluginSourceListRequest { node_id: None })?
            .sources
            .into_iter()
            .find(|source| source.id == *id)
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "The plugin source was not found.",
                )
            })
    }
}

fn rename(from: &std::path::Path, to: &std::path::Path) -> TalosResult<()> {
    std::fs::rename(from, to).map_err(|error| {
        TalosError::internal_with_source(
            "The prepared plugin source could not be published.",
            error,
        )
    })
}

#[cfg(test)]
#[path = "source_refresh_tests.rs"]
mod tests;
