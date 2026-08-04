use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json as BodyJson;
use talos_app_protocol::requests::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsResolveRequest,
    SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};
use talos_auth::RequestIdentity;
use talos_types::SkillsReadResponse;

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn list_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Query(params): Query<SkillsListRequest>,
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
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
) -> Result<JsonResponse<SkillsListResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .refresh_skills(&actor, Default::default())
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn suggest_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<RequestIdentity>,
    Query(params): Query<SkillsSuggestRequest>,
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
    BodyJson(params): BodyJson<SkillsReadRequest>,
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
    BodyJson(params): BodyJson<SkillsResolveRequest>,
) -> Result<JsonResponse<SkillsResolveResponse>, ApiError> {
    let actor = shared_actor(&auth.0);
    let response = state
        .gateway
        .resolve_skills(&actor, params)
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}
