use talos_config::{PluginsRuntimeConfig, SkillsRuntimeConfig};
use talos_error::TalosResult;
use talos_skills::{
    read_document, refresh, resolve, suggest, SkillDocument, SkillRoots, SkillSelector,
    SkillSnapshot, SkillSuggestion, SkillSummary, SkillWarning,
};
use talos_types::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsRefreshRequest,
    SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};

use crate::PluginManagerService;

#[derive(Clone)]
pub struct SkillsService {
    config: SkillsRuntimeConfig,
    plugins: PluginManagerService,
}

impl SkillsService {
    pub fn new(config: SkillsRuntimeConfig, plugins: PluginsRuntimeConfig) -> Self {
        Self {
            plugins: PluginManagerService::new(plugins, config.clone()),
            config,
        }
    }

    pub fn with_plugin_manager(mut self, plugin_manager: PluginManagerService) -> Self {
        self.plugins = plugin_manager;
        self
    }

    pub fn refresh(&self) -> TalosResult<SkillSnapshot> {
        refresh(&self.current_roots()?)
    }

    pub fn list(&self, include_disabled: bool) -> TalosResult<SkillSnapshot> {
        let mut snapshot = self.refresh()?;
        if !include_disabled {
            snapshot.skills.retain(|skill| skill.enabled);
        }
        Ok(snapshot)
    }

    pub fn list_with_request(&self, request: SkillsListRequest) -> TalosResult<SkillsListResponse> {
        self.snapshot_response(self.list(request.include_disabled)?)
    }

    pub fn refresh_with_request(
        &self,
        _request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.snapshot_response(self.refresh()?)
    }

    pub fn suggest(&self, query: &str, limit: usize) -> TalosResult<Vec<SkillSuggestion>> {
        suggest(&self.current_roots()?, query, limit)
    }

    pub fn suggest_with_request(
        &self,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        Ok(SkillsSuggestResponse {
            data: self.suggest(&request.query, request.limit.unwrap_or(20))?,
        })
    }

    pub fn read_document(&self, selector: &SkillSelector) -> TalosResult<SkillDocument> {
        read_document(&self.current_roots()?, selector)
    }

    pub fn read_with_request(&self, request: SkillsReadRequest) -> TalosResult<SkillDocument> {
        request.selector.validate().map_err(|error| {
            talos_error::TalosError::new(talos_error::ErrorCode::InvalidRequest, error.to_string())
        })?;
        self.read_document(&request.selector)
    }

    pub fn resolve(
        &self,
        text: &str,
        selected: &[SkillSelector],
    ) -> TalosResult<(Vec<SkillSummary>, Vec<SkillWarning>)> {
        resolve(&self.current_roots()?, text, selected)
    }

    pub fn resolve_with_request(
        &self,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        let (skills, warnings) = self.resolve(&request.text, &request.selected)?;
        Ok(SkillsResolveResponse {
            data: skills,
            warnings,
        })
    }

    fn snapshot_response(&self, snapshot: SkillSnapshot) -> TalosResult<SkillsListResponse> {
        Ok(SkillsListResponse {
            data: snapshot.skills,
            errors: snapshot.errors,
        })
    }

    fn current_roots(&self) -> TalosResult<SkillRoots> {
        self.plugins.skill_roots(&self.config)
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
