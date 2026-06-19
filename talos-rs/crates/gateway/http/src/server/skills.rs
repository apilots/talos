use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Json;
use axum::Json as BodyJson;
use talos_app_protocol::requests::{
    protocol_skill_document, protocol_skill_load_error, protocol_skill_selector,
    protocol_skill_suggestion, protocol_skill_summary, protocol_skill_warning,
    ProtocolSkillDocument, SkillsListRequest, SkillsListResponse, SkillsReadRequest,
    SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
};

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
        data: snapshot
            .skills
            .into_iter()
            .map(protocol_skill_summary)
            .collect(),
        errors: snapshot
            .errors
            .into_iter()
            .map(protocol_skill_load_error)
            .collect(),
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
        data: snapshot
            .skills
            .into_iter()
            .map(protocol_skill_summary)
            .collect(),
        errors: snapshot
            .errors
            .into_iter()
            .map(protocol_skill_load_error)
            .collect(),
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
    Ok(Json(SkillsSuggestResponse {
        data: skills.into_iter().map(protocol_skill_suggestion).collect(),
    }))
}

pub(super) async fn read_skill_handler(
    State(state): State<Arc<HttpApiState>>,
    BodyJson(params): BodyJson<SkillsReadRequest>,
) -> Result<Json<ProtocolSkillDocument>, ApiError> {
    let selector = protocol_skill_selector(params.selector).map_err(ApiError::from_talos_error)?;
    let document = state
        .app
        .skills
        .read_document(&selector)
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(protocol_skill_document(document)))
}

pub(super) async fn resolve_skills_handler(
    State(state): State<Arc<HttpApiState>>,
    BodyJson(params): BodyJson<SkillsResolveRequest>,
) -> Result<Json<SkillsResolveResponse>, ApiError> {
    let selected = params
        .selected
        .into_iter()
        .map(protocol_skill_selector)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::from_talos_error)?;
    let (skills, warnings) = state
        .app
        .skills
        .resolve(&params.text, &selected)
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(SkillsResolveResponse {
        data: skills.into_iter().map(protocol_skill_summary).collect(),
        warnings: warnings.into_iter().map(protocol_skill_warning).collect(),
    }))
}
