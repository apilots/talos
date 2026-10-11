use crate::gateway_service::ServiceGateway;
use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::*;
impl ServiceGateway {
    pub(crate) async fn list_skills(
        &self,
        actor: &Actor,
        request: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.service.list_skills(actor, request).await
    }
    pub(crate) async fn refresh_skills(
        &self,
        actor: &Actor,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.service.refresh_skills(actor, request).await
    }
    pub(crate) async fn suggest_skills(
        &self,
        actor: &Actor,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        self.service.suggest_skills(actor, request).await
    }
    pub(crate) async fn read_skill(
        &self,
        actor: &Actor,
        request: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        self.service.read_skill(actor, request).await
    }
    pub(crate) async fn resolve_skills(
        &self,
        actor: &Actor,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        self.service.resolve_skills(actor, request).await
    }
}
