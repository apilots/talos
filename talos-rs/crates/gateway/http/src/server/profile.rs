use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::response::Json;
use talos_auth::RequestIdentity;
use talos_types::{ProfileReadRequest, ProfileReadResponse};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn read_profile_handler(
    State(state): State<Arc<HttpApiState>>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<ProfileReadResponse>, ApiError> {
    let actor = shared_actor(&identity);
    let profile = state
        .gateway
        .read_profile(&actor, ProfileReadRequest {})
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(Json(profile))
}
