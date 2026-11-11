use super::preparation::StagingDirectory;
use super::*;
use crate::state::SourceScanCacheFile;

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
                if source.kind == MarketplaceSourceKind::LocalPackage {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "Local plugin packages cannot be refreshed. Add a new package with replacement instead.",
                    ));
                }
                let previous = self.state.load_source_scan_cache(&source.id)?;
                Ok((source, previous))
            })?;
        self.scan_and_publish_source(source, Some(previous))
    }

    pub(super) fn scan_and_publish_source(
        &self,
        source: MarketplaceSource,
        previous: Option<SourceScanCacheFile>,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        let directory = StagingDirectory::new(&self.state)?;
        let staging = self.state.staging_store(directory.path().join("source"));
        let scan = crate::scanner::scan_source(&staging, &source)?;
        self.publish_source_scan(source, previous, &directory, scan)
    }

    pub(super) fn publish_source_scan(
        &self,
        mut source: MarketplaceSource,
        previous: Option<SourceScanCacheFile>,
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
            &SourceScanCacheFile {
                version: 1,
                source_id: source.id.clone(),
                scanned_at_ms: source.updated_at_ms,
                source_commit: source.last_checked_commit.clone(),
                plugins: plugins.clone(),
                warnings: warnings.clone(),
            },
        )?;
        self.state.with_exclusive_lock("plugin.source.publish", || {
            let current = self.state.load_source_scan_cache(&source.id)?;
            let mut file = self.state.load_sources()?;
            let record = file
                .sources
                .iter_mut()
                .find(|record| record.id == source.id);
            if let Some(previous) = &previous {
                let current_source = self.source_by_id(&source.id)?;
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
            } else if let Some(record) = record.as_ref() {
                if !current.plugins.is_empty() {
                    return Ok(PluginSourceRefreshResponse {
                        source: (**record).clone(),
                        plugins: current.plugins,
                        warnings: current.warnings,
                    });
                }
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
            if let Some(record) = record {
                *record = source.clone();
                self.state.save_sources(&file)?;
            } else if previous.is_none() {
                file.sources.push(source.clone());
                if let Err(error) = self.state.save_sources(&file) {
                    rename(&target, &staging.marketplace_cache_dir(&source.id))?;
                    if backup.exists() {
                        rename(&backup, &target)?;
                    }
                    return Err(error);
                }
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
