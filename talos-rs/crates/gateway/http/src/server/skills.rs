use super::product_scope::{ProductJson, ProductQuery};
use std::sync::Arc;

use axum::extract::State;
use talos_auth::RequestIdentity;
use talos_types::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsReadResponse,
    SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn list_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    ProductQuery(params): ProductQuery<SkillsListRequest>,
) -> Result<JsonResponse<SkillsListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .list_skills(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn refresh_skills_handler(
    ProductQuery(params): ProductQuery<talos_types::SkillsRefreshRequest>,
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<SkillsListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .refresh_skills(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn suggest_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    ProductQuery(params): ProductQuery<SkillsSuggestRequest>,
) -> Result<JsonResponse<SkillsSuggestResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .suggest_skills(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn read_skill_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    ProductJson(params): ProductJson<SkillsReadRequest>,
) -> Result<JsonResponse<SkillsReadResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .read_skill(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn resolve_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    ProductJson(params): ProductJson<SkillsResolveRequest>,
) -> Result<JsonResponse<SkillsResolveResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .resolve_skills(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
