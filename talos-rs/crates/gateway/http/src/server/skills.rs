use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Json;
use axum::Json as BodyJson;
use talos_app_protocol::requests::{
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsResolveRequest,
    SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};
use talos_error::TalosError;
use talos_types::SkillDocument;

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn list_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    Query(params): Query<SkillsListRequest>,
) -> Result<Json<SkillsListResponse>, ApiError> {
    let snapshot = state
        .app
        .skills
        .list(params.include_disabled)
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(SkillsListResponse {
        data: snapshot.skills,
        errors: snapshot.errors,
    }))
}

pub(super) async fn refresh_skills_handler(
    State(state): State<Arc<HttpApiState>>,
) -> Result<Json<SkillsListResponse>, ApiError> {
    let snapshot = state
        .app
        .skills
        .refresh()
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(SkillsListResponse {
        data: snapshot.skills,
        errors: snapshot.errors,
    }))
}

pub(super) async fn suggest_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    Query(params): Query<SkillsSuggestRequest>,
) -> Result<Json<SkillsSuggestResponse>, ApiError> {
    let skills = state
        .app
        .skills
        .suggest(&params.query, params.limit.unwrap_or(20))
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(SkillsSuggestResponse { data: skills }))
}

pub(super) async fn read_skill_handler(
    State(state): State<Arc<HttpApiState>>,
    BodyJson(params): BodyJson<SkillsReadRequest>,
) -> Result<Json<SkillDocument>, ApiError> {
    params
        .selector
        .validate()
        .map_err(|err| ApiError::from_talos_error(TalosError::ValidationError(err.to_string())))?;
    let document = state
        .app
        .skills
        .read_document(&params.selector)
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(document))
}

pub(super) async fn resolve_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    BodyJson(params): BodyJson<SkillsResolveRequest>,
) -> Result<Json<SkillsResolveResponse>, ApiError> {
    let (skills, warnings) = state
        .app
        .skills
        .resolve(&params.text, &params.selected)
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(SkillsResolveResponse {
        data: skills,
        warnings,
    }))
}
