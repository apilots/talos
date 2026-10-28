use super::*;
use crate::installer::{content_digest_for_dir, copy_dir, plugin_source_dir, InstallSource};
use std::path::{Path, PathBuf};
use talos_types::{ContentDigest, PluginKind};

/// Immutable local candidate owned by one in-flight operation, never installation state.
pub struct PreparedPluginChange {
    candidate: Option<InstallCandidate>,
    target: Target,
    original: Option<PluginInstall>,
    digest: Option<ContentDigest>,
    _directory: Option<StagingDirectory>,
}

enum Target {
    Name { name: String, kind: PluginKind },
    Install(talos_types::PluginInstallId),
}

pub(super) struct StagingDirectory {
    path: PathBuf,
    _lease: talos_fs::FileLock,
}

impl StagingDirectory {
    pub(super) fn new(state: &PluginStateStore) -> TalosResult<Self> {
        state.with_exclusive_lock("plugin.preparation.allocate", || Self::new_locked(state))
    }
    pub(super) fn new_locked(state: &PluginStateStore) -> TalosResult<Self> {
        use talos_fs::FileSystem;
        let path = state
            .preparation_root()
            .join(uuid::Uuid::new_v4().to_string());
        talos_fs::create_directory(&path, true)?;
        let lease = talos_fs::std_fs().with_file_lock(&path.join("lease"))?;
        Ok(Self {
            path,
            _lease: lease,
        })
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        use talos_fs::FileSystem;
        if !self.path.exists() {
            return;
        }
        if let Err(error) = talos_fs::std_fs().remove_directory(&self.path) {
            talos_logging::report_error(&error);
        }
    }
}

impl PreparedPluginChange {
    /// Only a verified official Browser candidate can request Python dependencies.
    pub fn browser_requirements(&self) -> TalosResult<Option<PathBuf>> {
        let Some(candidate) = &self.candidate else {
            return Ok(None);
        };
        if candidate.plugin.id.0 != talos_types::OFFICIAL_BROWSER_PLUGIN_ID {
            return Ok(None);
        }
        super::official::validate_candidate(candidate)?;
        let directory = candidate.prepared_dir.as_ref().ok_or_else(conflict)?;
        let config =
            crate::scanner::mcp_manifest::browser_configuration(directory)?.ok_or_else(conflict)?;
        let lock = directory.join(config.docker_requirements);
        let contained = std::fs::canonicalize(&lock).map_err(|error| {
            TalosError::internal_with_source("The Browser dependency lock is unavailable.", error)
        })?;
        if !contained.is_file()
            || !contained.starts_with(std::fs::canonicalize(directory).map_err(|error| {
                TalosError::internal_with_source(
                    "The Browser candidate directory is unavailable.",
                    error,
                )
            })?)
        {
            return Err(conflict());
        }
        Ok(Some(contained))
    }

    fn verify(&self, manager: &PluginManager) -> TalosResult<()> {
        let installs = manager.state.load_installs()?;
        let current = installs.installs.iter().find(|install| match &self.target {
            Target::Name { name, kind } => install.name == *name && install.kind == *kind,
            Target::Install(id) => install.id == *id,
        });
        if current != self.original.as_ref() {
            return Err(conflict());
        }
        if let Some(candidate) = &self.candidate {
            let current = manager.install_candidate(&candidate.plugin.id)?;
            if current.plugin.source_commit != candidate.plugin.source_commit {
                return Err(conflict());
            }
            let InstallSource::Marketplace(source) = &current.source else {
                return Err(conflict());
            };
            let path = plugin_source_dir(&manager.state, source, &current.plugin)?;
            if Some(content_digest_for_dir(&path)?) != self.digest
                || Some(content_digest_for_dir(
                    candidate.prepared_dir.as_ref().ok_or_else(conflict)?,
                )?) != self.digest
            {
                return Err(conflict());
            }
        }
        Ok(())
    }
}

impl PluginManager {
    /// Copies source content under the existing file lock; no execution gate or network I/O.
    pub fn prepare_install(
        &self,
        request: &PluginInstallRequest,
    ) -> TalosResult<PreparedPluginChange> {
        self.state
            .with_exclusive_lock("plugin.install.prepare", || match &request.target {
                PluginInstallTarget::Plugin {
                    plugin_id,
                    expected_source_commit,
                } => {
                    let candidate = self.install_candidate(plugin_id)?;
                    if expected_source_commit.is_some()
                        && candidate.plugin.source_commit != *expected_source_commit
                    {
                        return Err(conflict());
                    }
                    let target = Target::Name {
                        name: candidate.plugin.metadata.name.clone(),
                        kind: candidate.plugin.kind,
                    };
                    self.snapshot_candidate(candidate, target)
                }
                PluginInstallTarget::ManualMcpServer { server, .. } => {
                    self.prepare_manual_mcp((**server).clone())?;
                    let target = Target::Name {
                        name: server.name.clone(),
                        kind: PluginKind::McpServer,
                    };
                    let original =
                        self.state
                            .load_installs()?
                            .installs
                            .into_iter()
                            .find(|install| {
                                install.kind == PluginKind::McpServer && install.name == server.name
                            });
                    Ok(PreparedPluginChange {
                        candidate: None,
                        target,
                        original,
                        digest: None,
                        _directory: None,
                    })
                }
            })
    }

    /// Source fetching completes before the final install transaction and all execution gates.
    pub fn prepare_update(
        &self,
        request: &PluginUpdateRequest,
    ) -> TalosResult<PreparedPluginChange> {
        let original = self
            .state
            .with_exclusive_lock("plugin.update.inspect", || {
                self.state
                    .load_installs()?
                    .installs
                    .into_iter()
                    .find(|install| install.id == request.install_id)
                    .ok_or_else(conflict)
            })?;
        let source_id = super::install_source_id(&original).ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "This plugin has no update source.",
            )
        })?;
        self.refresh_source(PluginSourceRefreshRequest {
            node_id: None,
            source_id,
        })?;
        self.state.with_exclusive_lock("plugin.update.prepare", || {
            let candidate = self.install_candidate(&original.plugin_id)?;
            if request.expected_source_commit.is_some()
                && candidate.plugin.source_commit != request.expected_source_commit
            {
                return Err(conflict());
            }
            let prepared =
                self.snapshot_candidate(candidate, Target::Install(request.install_id.clone()))?;
            if prepared.original.as_ref() != Some(&original) {
                return Err(conflict());
            }
            Ok(prepared)
        })
    }

    fn snapshot_candidate(
        &self,
        mut candidate: InstallCandidate,
        target: Target,
    ) -> TalosResult<PreparedPluginChange> {
        super::official::validate_candidate(&candidate)?;
        let original =
            self.state
                .load_installs()?
                .installs
                .into_iter()
                .find(|install| match &target {
                    Target::Name { name, kind } => install.name == *name && install.kind == *kind,
                    Target::Install(id) => install.id == *id,
                });
        let InstallSource::Marketplace(source) = &candidate.source else {
            return Err(conflict());
        };
        let directory = StagingDirectory::new_locked(&self.state)?;
        let root = directory.path().join("candidate");
        talos_fs::create_directory(&root, true)?;
        copy_dir(
            &plugin_source_dir(&self.state, source, &candidate.plugin)?,
            &root,
        )?;
        let digest = Some(content_digest_for_dir(&root)?);
        candidate.prepared_dir = Some(root);
        Ok(PreparedPluginChange {
            candidate: Some(candidate),
            target,
            original,
            digest,
            _directory: Some(directory),
        })
    }

    pub fn install_prepared(
        &self,
        request: PluginInstallRequest,
        mut prepared: PreparedPluginChange,
    ) -> TalosResult<PluginInstallResponse> {
        self.state.with_exclusive_lock("plugin.install.commit", || {
            prepared.verify(self)?;
            match (&request.target, &prepared.candidate, &prepared.target) {
                (PluginInstallTarget::Plugin { plugin_id, .. }, Some(candidate), _)
                    if *plugin_id == candidate.plugin.id => {}
                (
                    PluginInstallTarget::ManualMcpServer { server, .. },
                    None,
                    Target::Name { name, kind },
                ) if server.name == *name && *kind == PluginKind::McpServer => {}
                _ => return Err(conflict()),
            }
            match prepared.candidate.take() {
                Some(candidate) => {
                    self.activate_install(candidate, request.enable, request.conflict_policy)
                }
                None => self.install_locked(request),
            }
        })
    }

    pub fn update_prepared(
        &self,
        request: PluginUpdateRequest,
        mut prepared: PreparedPluginChange,
    ) -> TalosResult<PluginUpdateResponse> {
        self.state.with_exclusive_lock("plugin.update.commit", || {
            prepared.verify(self)?;
            if !matches!(&prepared.target, Target::Install(id) if *id == request.install_id) {
                return Err(conflict());
            }
            let candidate = prepared.candidate.take().ok_or_else(conflict)?;
            self.update_locked(request, candidate)
        })
    }
}

fn conflict() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::Conflict,
        "The plugin or source changed during preparation. Prepare it again before committing.",
    )
}

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
