use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsReadResponse,
    SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest,
    SkillsSuggestResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn list_skills(
        &self,
        _actor: &Actor,
        request: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.application_service.skills.list_with_request(request)
    }

    pub(crate) async fn refresh_skills(
        &self,
        _actor: &Actor,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.application_service
            .skills
            .refresh_with_request(request)
    }

    pub(crate) async fn suggest_skills(
        &self,
        _actor: &Actor,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        self.application_service
            .skills
            .suggest_with_request(request)
    }

    pub(crate) async fn read_skill(
        &self,
        _actor: &Actor,
        request: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        let document = self.application_service.skills.read_with_request(request)?;
        Ok(SkillsReadResponse { document })
    }

    pub(crate) async fn resolve_skills(
        &self,
        _actor: &Actor,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        self.application_service
            .skills
            .resolve_with_request(request)
    }
}
