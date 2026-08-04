use crate::installer::{
    commit_install_activation, commit_uninstall_activation, commit_update_activation,
    install_plugin, installed_mcp_servers, installed_plugin, installed_skill_root,
    missing_plugin_from_install, rollback_install_activation, rollback_uninstall_activation,
    rollback_update_activation, stage_uninstall_plugin, update_plugin, InstallCandidate,
};
use crate::scanner::{scan_source, scanned_at_ms};
use crate::source::{config_source, source_from_input};
use crate::state::SourceScanCacheFile;
use crate::PluginStateStore;
mod matchers;
mod read;
use matchers::{plugin_matches_list, plugin_matches_search};
use talos_config::{
    resolve_runtime_path, PluginRootKind as ConfigPluginRootKind, PluginsRuntimeConfig,
    SkillsRuntimeConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    Plugin, PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstall, PluginInstallRequest, PluginInstallResponse, PluginInstallTarget,
    PluginListRequest, PluginListResponse, PluginSearchRefreshPolicy, PluginSearchRequest,
    PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginStatus, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse, PluginWarning,
    PluginWarningCode,
};

#[derive(Debug, Clone)]
pub struct PluginManager {
    state: PluginStateStore,
    config: PluginsRuntimeConfig,
    max_skill_md_bytes: usize,
}

impl PluginManager {
    pub fn new(config: PluginsRuntimeConfig) -> Self {
        Self {
            state: PluginStateStore::from_runtime_home(),
            config,
            max_skill_md_bytes: SkillsRuntimeConfig::default().max_skill_md_bytes,
        }
    }

    pub fn with_state_store(config: PluginsRuntimeConfig, state: PluginStateStore) -> Self {
        Self {
            state,
            config,
            max_skill_md_bytes: SkillsRuntimeConfig::default().max_skill_md_bytes,
        }
    }

    pub fn with_max_skill_md_bytes(mut self, max_skill_md_bytes: usize) -> Self {
        self.max_skill_md_bytes = max_skill_md_bytes;
        self
    }

    pub fn list_sources(
        &self,
        _request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        let mut sources = self.config_sources();
        sources.extend(self.state.load_sources()?.sources);
        sources.sort_by(|left, right| {
            left.source_type
                .cmp(&right.source_type)
                .then_with(|| left.display_name.cmp(&right.display_name))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(PluginSourceListResponse { sources })
    }

    pub fn add_source(
        &self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.state
            .with_exclusive_lock("plugin.source.add", || self.add_source_locked(request))
    }

    fn add_source_locked(
        &self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        let source = source_from_input(request.source)?;
        let existing = self.list_sources(PluginSourceListRequest {})?;
        if existing.sources.iter().any(|item| item.id == source.id) {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("plugin source `{}` already exists", source.id.0),
            ));
        }

        let mut file = self.state.load_sources()?;
        file.sources.push(source.clone());
        self.state.save_sources(&file)?;

        if request.refresh {
            let refreshed = self.refresh_source_locked(PluginSourceRefreshRequest {
                source_id: source.id,
            })?;
            return Ok(PluginSourceAddResponse {
                source: refreshed.source,
                plugins: refreshed.plugins,
                warnings: refreshed.warnings,
            });
        }

        Ok(PluginSourceAddResponse {
            source,
            plugins: Vec::new(),
            warnings: Vec::new(),
        })
    }

    pub fn refresh_source(
        &self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.state.with_exclusive_lock("plugin.source.refresh", || {
            self.refresh_source_locked(request)
        })
    }

    fn refresh_source_locked(
        &self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        if let Some(mut source) = self
            .config_sources()
            .into_iter()
            .find(|source| source.id == request.source_id)
        {
            let scan = scan_source(&self.state, &source)?;
            source.last_checked_commit = scan.source_commit.clone();
            source.last_checked_at_ms = Some(scanned_at_ms());
            source.updated_at_ms = source.last_checked_at_ms.unwrap_or_default();
            let warnings = scan.warnings;
            let plugins =
                plugins_with_source_commit(scan.plugins, source.last_checked_commit.clone());
            self.save_scan_cache(&source, plugins.clone(), warnings.clone())?;
            return Ok(PluginSourceRefreshResponse {
                source,
                plugins,
                warnings,
            });
        }

        let mut file = self.state.load_sources()?;
        let Some(source) = file
            .sources
            .iter_mut()
            .find(|source| source.id == request.source_id)
        else {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("plugin source `{}` not found", request.source_id.0),
            ));
        };
        let scan = scan_source(&self.state, source)?;
        source.last_checked_commit = scan.source_commit.clone();
        source.last_checked_at_ms = Some(scanned_at_ms());
        source.updated_at_ms = source.last_checked_at_ms.unwrap_or_default();
        let warnings = scan.warnings;
        let plugins = plugins_with_source_commit(scan.plugins, source.last_checked_commit.clone());
        self.save_scan_cache(source, plugins.clone(), warnings.clone())?;
        let source = source.clone();
        self.state.save_sources(&file)?;

        Ok(PluginSourceRefreshResponse {
            source,
            plugins,
            warnings,
        })
    }

    pub fn search(&self, request: PluginSearchRequest) -> TalosResult<PluginSearchResponse> {
        let mut plugins = Vec::new();
        let mut warnings = Vec::new();
        for source in self.list_sources(PluginSourceListRequest {})?.sources {
            if request
                .source_id
                .as_ref()
                .is_some_and(|source_id| source_id != &source.id)
            {
                continue;
            }
            let cached = self.state.load_source_scan_cache(&source.id)?;
            let should_refresh = match request.refresh {
                PluginSearchRefreshPolicy::CacheOnly => false,
                PluginSearchRefreshPolicy::RefreshStale => cached.plugins.is_empty(),
                PluginSearchRefreshPolicy::ForceRefresh => true,
            };
            let source_plugins = if should_refresh {
                match self.refresh_source(PluginSourceRefreshRequest {
                    source_id: source.id.clone(),
                }) {
                    Ok(refreshed) => {
                        warnings.extend(refreshed.warnings);
                        refreshed.plugins
                    }
                    Err(err) => {
                        warnings.push(PluginWarning {
                            code: PluginWarningCode::SourceRefreshFailed,
                            message: format!(
                                "failed to refresh plugin source `{}`: {err}",
                                source.id.0
                            ),
                            kind: None,
                            source_id: Some(source.id.clone()),
                            path: None,
                        });
                        warnings.extend(cached.warnings);
                        cached.plugins
                    }
                }
            } else {
                warnings.extend(cached.warnings);
                cached.plugins
            };
            plugins.extend(source_plugins);
        }
        self.merge_installed_statuses(&mut plugins)?;
        plugins.retain(|plugin| plugin_matches_search(plugin, &request));
        plugins.sort_by(|left, right| {
            left.metadata
                .name
                .cmp(&right.metadata.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        if let Some(limit) = request.limit {
            plugins.truncate(limit);
        }
        Ok(PluginSearchResponse { plugins, warnings })
    }

    pub fn list_plugins(&self, request: PluginListRequest) -> TalosResult<PluginListResponse> {
        self.state
            .with_exclusive_lock("plugin.list", || self.list_plugins_locked(request))
    }

    fn list_plugins_locked(&self, request: PluginListRequest) -> TalosResult<PluginListResponse> {
        self.state.cleanup_install_temps()?;
        let installs = self.state.load_installs()?.installs;
        let warnings = Vec::new();
        let mut plugins = Vec::new();
        for install in installs {
            if request.kind.is_some_and(|kind| kind != install.kind) {
                continue;
            }
            plugins.push(self.plugin_for_install(&install, install.enabled));
        }
        plugins.retain(|plugin| plugin_matches_list(plugin, &request));
        plugins.sort_by(|left, right| {
            left.metadata
                .name
                .cmp(&right.metadata.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(PluginListResponse { plugins, warnings })
    }

    pub fn install(&self, request: PluginInstallRequest) -> TalosResult<PluginInstallResponse> {
        self.state
            .with_exclusive_lock("plugin.install", || self.install_locked(request))
    }

    fn install_locked(&self, request: PluginInstallRequest) -> TalosResult<PluginInstallResponse> {
        let PluginInstallTarget::Plugin {
            plugin_id,
            expected_source_commit,
        } = request.target;
        let candidate = self.install_candidate(&plugin_id)?;
        if expected_source_commit.is_some()
            && candidate.plugin.source_commit != expected_source_commit
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("plugin `{}` source commit changed", plugin_id.0),
            ));
        }
        let enabled = request.enable != Some(false);
        let activation = install_plugin(&self.state, candidate, request.conflict_policy, enabled)?;
        let install = activation.install.clone();
        let previous_installs = self.state.load_installs()?;
        let mut installs = previous_installs.clone();
        installs
            .installs
            .retain(|item| item.id != install.id && item.installed_path != install.installed_path);
        installs.installs.push(install.clone());
        if let Err(err) = self.state.save_installs(&installs) {
            let _ = rollback_install_activation(&self.state, &activation);
            return Err(err);
        }
        if let Err(err) = commit_install_activation(&activation) {
            return Err(err);
        }
        let plugin = self.plugin_for_install(&install, enabled);
        Ok(PluginInstallResponse { plugin, install })
    }

    pub fn update(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        self.state
            .with_exclusive_lock("plugin.update", || self.update_locked(request))
    }

    fn update_locked(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        let install = self.install_by_id(&request.install_id)?;
        let enabled = install.enabled;
        let candidate = self.update_candidate(&install)?;
        if request.expected_source_commit.is_some()
            && candidate.plugin.source_commit != request.expected_source_commit
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("plugin install `{}` source commit changed", install.id.0),
            ));
        }
        let activation = update_plugin(
            &self.state,
            &install,
            candidate,
            request.conflict_policy,
            enabled,
        )?;
        let updated = activation.install.clone();
        let mut installs = self.state.load_installs()?;
        installs.installs.retain(|item| item.id != updated.id);
        installs.installs.push(updated.clone());
        if let Err(err) = self.state.save_installs(&installs) {
            let _ = rollback_update_activation(&self.state, &activation);
            return Err(err);
        }
        commit_update_activation(&activation)?;
        Ok(PluginUpdateResponse {
            plugin: activation.plugin,
            install: updated,
        })
    }

    pub fn uninstall(
        &self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.state
            .with_exclusive_lock("plugin.uninstall", || self.uninstall_locked(request))
    }

    fn uninstall_locked(
        &self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        let mut installs = self.state.load_installs()?;
        let original_len = installs.installs.len();
        let removed_install = installs
            .installs
            .iter()
            .find(|install| install.id == request.install_id)
            .cloned();
        installs
            .installs
            .retain(|install| install.id != request.install_id);
        let removed_record = installs.installs.len() != original_len;
        let activation = match removed_install.as_ref() {
            Some(install) => stage_uninstall_plugin(&self.state, install)?,
            None => {
                return Ok(PluginUninstallResponse {
                    install_id: request.install_id,
                    uninstalled: false,
                });
            }
        };
        if removed_record {
            if let Err(err) = self.state.save_installs(&installs) {
                let _ = rollback_uninstall_activation(&self.state, &activation);
                return Err(err);
            }
        }
        commit_uninstall_activation(&activation)?;
        Ok(PluginUninstallResponse {
            install_id: request.install_id,
            uninstalled: removed_record || activation.removed_path,
        })
    }

    pub fn enable(&self, request: PluginEnableRequest) -> TalosResult<PluginEnableResponse> {
        self.state
            .with_exclusive_lock("plugin.enable", || self.enable_locked(request))
    }

    fn enable_locked(&self, request: PluginEnableRequest) -> TalosResult<PluginEnableResponse> {
        let install = self.set_install_enabled(&request.install_id, true)?;
        Ok(PluginEnableResponse {
            plugin: self.plugin_for_install(&install, true),
        })
    }

    pub fn disable(&self, request: PluginDisableRequest) -> TalosResult<PluginDisableResponse> {
        self.state
            .with_exclusive_lock("plugin.disable", || self.disable_locked(request))
    }

    fn disable_locked(&self, request: PluginDisableRequest) -> TalosResult<PluginDisableResponse> {
        let install = self.set_install_enabled(&request.install_id, false)?;
        Ok(PluginDisableResponse {
            plugin: self.plugin_for_install(&install, false),
        })
    }

    pub fn remove_source(
        &self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.state.with_exclusive_lock("plugin.source.remove", || {
            self.remove_source_locked(request)
        })
    }

    fn remove_source_locked(
        &self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        if self
            .config_sources()
            .iter()
            .any(|source| source.id == request.source_id)
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "configured plugin source `{}` cannot be removed",
                    request.source_id.0
                ),
            ));
        }

        let mut file = self.state.load_sources()?;
        let original_len = file.sources.len();
        file.sources.retain(|source| source.id != request.source_id);
        let removed = file.sources.len() != original_len;
        if removed {
            self.state.save_sources(&file)?;
            self.state.remove_marketplace_cache(&request.source_id)?;
        }

        Ok(PluginSourceRemoveResponse {
            source_id: request.source_id,
            removed,
        })
    }

    pub fn skill_roots(
        &self,
        config: &SkillsRuntimeConfig,
    ) -> TalosResult<talos_skills::SkillRoots> {
        self.state.with_exclusive_lock("plugin.skill_roots", || {
            self.state.cleanup_install_temps()?;
            let mut roots = self.configured_skill_root_entries();
            roots.extend(
                self.state
                    .load_installs()?
                    .installs
                    .iter()
                    .filter_map(installed_skill_root),
            );
            Ok(talos_skills::SkillRoots {
                roots,
                state_path: self.state.skills_state_path(),
                max_skill_md_bytes: config.max_skill_md_bytes,
                max_available_summary_bytes: config.max_available_summary_bytes,
            })
        })
    }

    pub fn mcp_servers(&self) -> TalosResult<Vec<talos_types::McpServerRuntimeDescriptor>> {
        self.state.with_exclusive_lock("plugin.mcp_servers", || {
            self.state.cleanup_install_temps()?;
            let mut servers = Vec::new();
            for install in self.state.load_installs()?.installs {
                if install.kind != talos_types::PluginKind::McpServer || !install.enabled {
                    continue;
                }
                servers.extend(installed_mcp_servers(&install)?);
            }
            servers.sort_by(|left, right| {
                left.server
                    .name
                    .cmp(&right.server.name)
                    .then_with(|| format!("{:?}", left.source).cmp(&format!("{:?}", right.source)))
            });
            Ok(servers)
        })
    }

    fn configured_skill_root_entries(&self) -> Vec<talos_skills::SkillRoot> {
        self.config
            .roots
            .iter()
            .filter(|root| root.kind == ConfigPluginRootKind::Skill && root.read_only)
            .map(|root| talos_skills::SkillRoot::configured(resolve_runtime_path(&root.path)))
            .collect()
    }

    fn config_sources(&self) -> Vec<talos_types::MarketplaceSource> {
        self.config
            .sources
            .iter()
            .filter_map(config_source)
            .collect()
    }

    fn save_scan_cache(
        &self,
        source: &talos_types::MarketplaceSource,
        plugins: Vec<Plugin>,
        warnings: Vec<talos_types::PluginWarning>,
    ) -> TalosResult<()> {
        self.state.save_source_scan_cache(
            &source.id,
            &SourceScanCacheFile {
                version: 1,
                source_id: source.id.clone(),
                scanned_at_ms: source.updated_at_ms,
                source_commit: source.last_checked_commit.clone(),
                plugins,
                warnings,
            },
        )
    }

    fn install_candidate(
        &self,
        plugin_id: &talos_types::PluginId,
    ) -> TalosResult<InstallCandidate> {
        for source in self.list_sources(PluginSourceListRequest {})?.sources {
            let cache = self.state.load_source_scan_cache(&source.id)?;
            if let Some(plugin) = cache
                .plugins
                .into_iter()
                .find(|plugin| plugin.id == *plugin_id)
            {
                return Ok(InstallCandidate { plugin, source });
            }
        }
        Err(TalosError::public(
            talos_error::ErrorCode::ResourceNotFound,
            format!("plugin `{}` not found in marketplace cache", plugin_id.0),
        ))
    }

    fn install_by_id(
        &self,
        install_id: &talos_types::PluginInstallId,
    ) -> TalosResult<PluginInstall> {
        self.state
            .load_installs()?
            .installs
            .into_iter()
            .find(|install| &install.id == install_id)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("plugin install `{}` not found", install_id.0),
                )
            })
    }

    fn plugin_for_install(&self, install: &PluginInstall, enabled: bool) -> Plugin {
        if !install.installed_path.exists() {
            let mut plugin = missing_plugin_from_install(install, enabled);
            plugin.status = PluginStatus::Conflict {
                reason: talos_types::PluginConflictReason::TargetPathMissing,
            };
            return plugin;
        }
        let Some(plugin) = self.find_cached_plugin(&install.plugin_id) else {
            return missing_plugin_from_install(install, enabled);
        };
        let latest_commit = plugin
            .source_commit
            .as_ref()
            .filter(|commit| !commit.is_empty())
            .cloned();
        match (installed_commit(install), latest_commit) {
            (Some(installed_commit), Some(latest_commit)) if installed_commit != &latest_commit => {
                let mut plugin = installed_plugin(plugin, install, enabled);
                plugin.status = PluginStatus::UpdateAvailable {
                    install_id: install.id.clone(),
                    installed_path: install.installed_path.clone(),
                    installed_commit: Some(installed_commit.clone()),
                    latest_commit,
                    enabled,
                };
                plugin
            }
            _ => installed_plugin(plugin, install, enabled),
        }
    }

    fn find_cached_plugin(&self, plugin_id: &talos_types::PluginId) -> Option<Plugin> {
        let sources = self.list_sources(PluginSourceListRequest {}).ok()?;
        for source in sources.sources {
            let Ok(cache) = self.state.load_source_scan_cache(&source.id) else {
                continue;
            };
            if let Some(plugin) = cache
                .plugins
                .into_iter()
                .find(|plugin| plugin.id == *plugin_id)
            {
                return Some(plugin);
            }
        }
        None
    }

    fn set_install_enabled(
        &self,
        install_id: &talos_types::PluginInstallId,
        enabled: bool,
    ) -> TalosResult<PluginInstall> {
        let mut installs = self.state.load_installs()?;
        let install = installs
            .installs
            .iter_mut()
            .find(|install| &install.id == install_id)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("plugin install `{}` not found", install_id.0),
                )
            })?;
        install.enabled = enabled;
        let install = install.clone();
        self.state.save_installs(&installs)?;
        Ok(install)
    }

    fn merge_installed_statuses(&self, plugins: &mut [Plugin]) -> TalosResult<()> {
        let installs = self.state.load_installs()?.installs;
        for plugin in plugins {
            let Some(install) = installs
                .iter()
                .find(|install| install.plugin_id == plugin.id)
            else {
                continue;
            };
            let enabled = install.enabled;
            if !install.installed_path.exists() {
                plugin.status = PluginStatus::Conflict {
                    reason: talos_types::PluginConflictReason::TargetPathMissing,
                };
                continue;
            }
            let latest_commit = plugin
                .source_commit
                .as_ref()
                .filter(|commit| !commit.is_empty());
            match (installed_commit(install), latest_commit) {
                (Some(installed_commit), Some(latest_commit))
                    if installed_commit != latest_commit =>
                {
                    plugin.status = PluginStatus::UpdateAvailable {
                        install_id: install.id.clone(),
                        installed_path: install.installed_path.clone(),
                        installed_commit: Some(installed_commit.clone()),
                        latest_commit: latest_commit.clone(),
                        enabled,
                    };
                }
                _ => {
                    plugin.status = PluginStatus::Installed {
                        install_id: install.id.clone(),
                        installed_path: install.installed_path.clone(),
                        installed_commit: installed_commit(install).cloned(),
                        enabled,
                    };
                }
            }
        }
        Ok(())
    }

    fn update_candidate(&self, install: &PluginInstall) -> TalosResult<InstallCandidate> {
        let source_id = install_source_id(install).ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("plugin install `{}` has no update source", install.id.0),
            )
        })?;
        let refreshed = self.refresh_source_locked(PluginSourceRefreshRequest { source_id })?;
        let plugin = refreshed
            .plugins
            .into_iter()
            .find(|plugin| plugin.id == install.plugin_id)
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!(
                        "plugin `{}` not found in refreshed source",
                        install.plugin_id.0
                    ),
                )
            })?;
        Ok(InstallCandidate {
            plugin,
            source: refreshed.source,
        })
    }
}

fn plugins_with_source_commit(mut plugins: Vec<Plugin>, commit: Option<String>) -> Vec<Plugin> {
    for plugin in &mut plugins {
        plugin.source_commit = commit.clone();
    }
    plugins
}

fn install_source_id(install: &PluginInstall) -> Option<talos_types::MarketplaceSourceId> {
    match &install.source {
        talos_types::PluginInstallSource::Marketplace { source_id, .. } => Some(source_id.clone()),
        talos_types::PluginInstallSource::LocalPath { source_id, .. } => Some(source_id.clone()),
        talos_types::PluginInstallSource::Manual => None,
    }
}

fn installed_commit(install: &PluginInstall) -> Option<&String> {
    match &install.source {
        talos_types::PluginInstallSource::Marketplace { commit, .. } => Some(commit),
        talos_types::PluginInstallSource::LocalPath { .. }
        | talos_types::PluginInstallSource::Manual => None,
    }
}
