use crate::installer::{
    install_plugin, installed_plugin, installed_skill_root, missing_plugin_from_install,
    uninstall_plugin, update_plugin, InstallCandidate,
};
use crate::scanner::{scan_source, scanned_at_ms};
use crate::source::{config_source, source_from_input};
use crate::state::SourceScanCacheFile;
use crate::PluginStateStore;
use std::collections::BTreeSet;
use talos_config::{
    resolve_runtime_path, PluginRootKind as ConfigPluginRootKind, PluginsRuntimeConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_skills::{load_skill_state, save_skill_state, PluginSkillStateKey};
use talos_types::{
    Plugin, PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstall, PluginInstallRequest, PluginInstallResponse, PluginInstallTarget,
    PluginListRequest, PluginListResponse, PluginRootConfig, PluginRootKind,
    PluginSearchRefreshPolicy, PluginSearchRequest, PluginSearchResponse, PluginSearchStatusFilter,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginStatus, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse, PluginWarning,
    PluginWarningCode,
};

#[derive(Debug, Clone)]
pub struct PluginManager {
    state: PluginStateStore,
    config: PluginsRuntimeConfig,
}

impl PluginManager {
    pub fn new(config: PluginsRuntimeConfig) -> Self {
        Self {
            state: PluginStateStore::from_runtime_home(),
            config,
        }
    }

    pub fn with_state_store(config: PluginsRuntimeConfig, state: PluginStateStore) -> Self {
        Self { state, config }
    }

    pub fn list_sources(
        &self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        let mut sources = self.config_sources();
        sources.extend(self.state.load_sources()?.sources);
        if !request.include_disabled {
            sources.retain(|source| source.enabled);
        }
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
        let existing = self.list_sources(PluginSourceListRequest {
            include_disabled: true,
        })?;
        if existing.sources.iter().any(|item| item.id == source.id) {
            return Err(TalosError::Conflict(format!(
                "plugin source `{}` already exists",
                source.id.0
            )));
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
            return Err(TalosError::ResourceNotFound(format!(
                "plugin source `{}` not found",
                request.source_id.0
            )));
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
        for source in self
            .list_sources(PluginSourceListRequest {
                include_disabled: false,
            })?
            .sources
        {
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
                let refreshed = self.refresh_source(PluginSourceRefreshRequest {
                    source_id: source.id.clone(),
                })?;
                warnings.extend(refreshed.warnings);
                refreshed.plugins
            } else {
                warnings.extend(cached.warnings);
                cached.plugins
            };
            plugins.extend(source_plugins);
        }
        self.merge_installed_statuses(&mut plugins)?;
        plugins.retain(|plugin| plugin_matches_search(plugin, &request));
        plugins.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
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
        let warnings = self.orphan_install_warnings(&installs)?;
        let mut plugins = Vec::new();
        for install in installs {
            if request.kind.is_some_and(|kind| kind != install.kind) {
                continue;
            }
            let enabled = self.install_enabled(&install)?;
            plugins.push(self.plugin_for_install(&install, enabled));
        }
        plugins.retain(|plugin| plugin_matches_list(plugin, &request));
        plugins.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
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
            return Err(TalosError::Conflict(format!(
                "plugin `{}` source commit changed",
                plugin_id.0
            )));
        }
        let (_, install) = install_plugin(&self.state, candidate, request.conflict_policy)?;
        let mut installs = self.state.load_installs()?;
        installs.installs.retain(|item| item.id != install.id);
        installs.installs.push(install.clone());
        self.state.save_installs(&installs)?;
        let enabled = request.enable != Some(false);
        self.set_install_enabled(&install, enabled)?;
        let plugin = self.plugin_for_install(&install, enabled);
        Ok(PluginInstallResponse { plugin, install })
    }

    pub fn update(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        self.state
            .with_exclusive_lock("plugin.update", || self.update_locked(request))
    }

    fn update_locked(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        let install = self.install_by_id(&request.install_id)?;
        let enabled = self.install_enabled(&install)?;
        let candidate = self.update_candidate(&install)?;
        if request.expected_source_commit.is_some()
            && candidate.plugin.source_commit != request.expected_source_commit
        {
            return Err(TalosError::Conflict(format!(
                "plugin install `{}` source commit changed",
                install.id.0
            )));
        }
        let (plugin, updated) = update_plugin(
            &self.state,
            &install,
            candidate,
            request.conflict_policy,
            enabled,
        )?;
        let mut installs = self.state.load_installs()?;
        installs.installs.retain(|item| item.id != updated.id);
        installs.installs.push(updated.clone());
        self.state.save_installs(&installs)?;
        Ok(PluginUpdateResponse {
            plugin,
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
        let removed_path = uninstall_plugin(&self.state, &request.install_id)?;
        if removed_record {
            self.state.save_installs(&installs)?;
            if let Some(install) = removed_install {
                self.set_install_enabled(&install, true)?;
            }
        }
        Ok(PluginUninstallResponse {
            install_id: request.install_id,
            uninstalled: removed_record || removed_path,
        })
    }

    pub fn enable(&self, request: PluginEnableRequest) -> TalosResult<PluginEnableResponse> {
        self.state
            .with_exclusive_lock("plugin.enable", || self.enable_locked(request))
    }

    fn enable_locked(&self, request: PluginEnableRequest) -> TalosResult<PluginEnableResponse> {
        let install = self.install_by_id(&request.install_id)?;
        self.set_install_enabled(&install, true)?;
        Ok(PluginEnableResponse {
            plugin: self.plugin_for_install(&install, true),
        })
    }

    pub fn disable(&self, request: PluginDisableRequest) -> TalosResult<PluginDisableResponse> {
        self.state
            .with_exclusive_lock("plugin.disable", || self.disable_locked(request))
    }

    fn disable_locked(&self, request: PluginDisableRequest) -> TalosResult<PluginDisableResponse> {
        let install = self.install_by_id(&request.install_id)?;
        self.set_install_enabled(&install, false)?;
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
            return Err(TalosError::ValidationError(format!(
                "configured plugin source `{}` cannot be removed",
                request.source_id.0
            )));
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

    pub fn configured_skill_roots(&self) -> Vec<PluginRootConfig> {
        self.config
            .roots
            .iter()
            .filter(|root| root.kind == ConfigPluginRootKind::Skill && root.read_only)
            .map(|root| PluginRootConfig {
                kind: PluginRootKind::Skill,
                path: resolve_runtime_path(&root.path),
                read_only: true,
            })
            .collect()
    }

    pub fn installed_skill_roots(&self) -> TalosResult<Vec<talos_skills::SkillRoot>> {
        self.state
            .with_exclusive_lock("plugin.installed_skill_roots", || {
                self.installed_skill_roots_locked()
            })
    }

    fn installed_skill_roots_locked(&self) -> TalosResult<Vec<talos_skills::SkillRoot>> {
        self.state.cleanup_install_temps()?;
        Ok(self
            .state
            .load_installs()?
            .installs
            .iter()
            .filter_map(installed_skill_root)
            .collect())
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
        for source in self
            .list_sources(PluginSourceListRequest {
                include_disabled: false,
            })?
            .sources
        {
            let cache = self.state.load_source_scan_cache(&source.id)?;
            if let Some(plugin) = cache
                .plugins
                .into_iter()
                .find(|plugin| plugin.id == *plugin_id)
            {
                return Ok(InstallCandidate { plugin, source });
            }
        }
        Err(TalosError::ResourceNotFound(format!(
            "plugin `{}` not found in marketplace cache",
            plugin_id.0
        )))
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
                TalosError::ResourceNotFound(format!("plugin install `{}` not found", install_id.0))
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
        let sources = self
            .list_sources(PluginSourceListRequest {
                include_disabled: true,
            })
            .ok()?;
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

    fn install_enabled(&self, install: &PluginInstall) -> TalosResult<bool> {
        let state = load_skill_state(&self.state.skills_state_path())?;
        Ok(!state.is_disabled(Some(&install_state_key(install))))
    }

    fn set_install_enabled(&self, install: &PluginInstall, enabled: bool) -> TalosResult<()> {
        let path = self.state.skills_state_path();
        let mut state = load_skill_state(&path)?;
        let key = install_state_key(install);
        if enabled {
            state.enable(&key);
        } else {
            state.disable(key);
        }
        save_skill_state(&path, &state)
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
            let enabled = self.install_enabled(install)?;
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
            TalosError::ValidationError(format!(
                "plugin install `{}` has no update source",
                install.id.0
            ))
        })?;
        let refreshed = self.refresh_source_locked(PluginSourceRefreshRequest { source_id })?;
        let plugin = refreshed
            .plugins
            .into_iter()
            .find(|plugin| plugin.id == install.plugin_id)
            .ok_or_else(|| {
                TalosError::ResourceNotFound(format!(
                    "plugin `{}` not found in refreshed source",
                    install.plugin_id.0
                ))
            })?;
        Ok(InstallCandidate {
            plugin,
            source: refreshed.source,
        })
    }

    fn orphan_install_warnings(
        &self,
        installs: &[PluginInstall],
    ) -> TalosResult<Vec<PluginWarning>> {
        let installs_dir = self.state.installs_dir();
        if !installs_dir.exists() {
            return Ok(Vec::new());
        }
        let known = installs
            .iter()
            .map(|install| install.id.0.as_str())
            .collect::<BTreeSet<_>>();
        let mut warnings = Vec::new();
        for entry in std::fs::read_dir(&installs_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read plugin installs dir `{}`: {err}",
                installs_dir.display()
            ))
        })? {
            let entry = entry.map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to read plugin installs dir `{}` entry: {err}",
                    installs_dir.display()
                ))
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".tmp-")
                || name.starts_with(".backup-")
                || known.contains(name.as_str())
            {
                continue;
            }
            warnings.push(PluginWarning {
                code: PluginWarningCode::OrphanInstallDirectory,
                message: format!(
                    "plugin install directory `{}` has no install record",
                    path.display()
                ),
                kind: None,
                source_id: None,
                path: Some(path.to_string_lossy().to_string()),
            });
        }
        warnings.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(warnings)
    }
}

fn plugins_with_source_commit(mut plugins: Vec<Plugin>, commit: Option<String>) -> Vec<Plugin> {
    for plugin in &mut plugins {
        plugin.source_commit = commit.clone();
    }
    plugins
}

fn plugin_matches_search(plugin: &Plugin, request: &PluginSearchRequest) -> bool {
    if request.kind.is_some_and(|kind| kind != plugin.kind) {
        return false;
    }
    if !matches_status(plugin, request.status) {
        return false;
    }
    if let Some(query) = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        let query = query.to_lowercase();
        return plugin.name.to_lowercase().contains(&query)
            || plugin.description.to_lowercase().contains(&query);
    }
    true
}

fn matches_status(plugin: &Plugin, status: Option<PluginSearchStatusFilter>) -> bool {
    match status.unwrap_or(PluginSearchStatusFilter::Any) {
        PluginSearchStatusFilter::Any => true,
        PluginSearchStatusFilter::NotInstalled => {
            matches!(plugin.status, PluginStatus::NotInstalled)
        }
        PluginSearchStatusFilter::Installed => {
            matches!(plugin.status, PluginStatus::Installed { .. })
        }
        PluginSearchStatusFilter::UpdateAvailable => {
            matches!(plugin.status, PluginStatus::UpdateAvailable { .. })
        }
        PluginSearchStatusFilter::Conflict => {
            matches!(plugin.status, PluginStatus::Conflict { .. })
        }
    }
}

fn plugin_matches_list(plugin: &Plugin, request: &PluginListRequest) -> bool {
    if request
        .source_id
        .as_ref()
        .is_some_and(|source_id| plugin.source_id.as_ref() != Some(source_id))
    {
        return false;
    }
    match request.status {
        Some(talos_types::PluginListStatusFilter::Installed) => {
            matches!(plugin.status, PluginStatus::Installed { enabled: true, .. })
        }
        Some(talos_types::PluginListStatusFilter::Disabled) => {
            matches!(
                plugin.status,
                PluginStatus::Installed { enabled: false, .. }
            )
        }
        Some(talos_types::PluginListStatusFilter::UpdateAvailable) => {
            matches!(plugin.status, PluginStatus::UpdateAvailable { .. })
        }
        Some(talos_types::PluginListStatusFilter::SourceMissing) => {
            matches!(plugin.status, PluginStatus::SourceMissing { .. })
        }
        Some(talos_types::PluginListStatusFilter::Conflict) => {
            matches!(plugin.status, PluginStatus::Conflict { .. })
        }
        None => true,
    }
}

fn install_state_key(install: &PluginInstall) -> PluginSkillStateKey {
    PluginSkillStateKey {
        install_id: install.id.0.clone(),
        relative_skill_md_path: "SKILL.md".into(),
    }
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
