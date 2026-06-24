use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsReadResponse,
    SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest,
    SkillsSuggestResponse,
};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn list_skills(
        &self,
        _actor: &Actor,
        request: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.service.skills.list_with_request(request)
    }

    pub(crate) async fn refresh_skills(
        &self,
        _actor: &Actor,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.service.skills.refresh_with_request(request)
    }

    pub(crate) async fn suggest_skills(
        &self,
        _actor: &Actor,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        self.service.skills.suggest_with_request(request)
    }

    pub(crate) async fn read_skill(
        &self,
        _actor: &Actor,
        request: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        let document = self.service.skills.read_with_request(request)?;
        Ok(SkillsReadResponse { document })
    }

    pub(crate) async fn resolve_skills(
        &self,
        _actor: &Actor,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        self.service.skills.resolve_with_request(request)
    }
}
