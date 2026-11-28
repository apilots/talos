//! Typed product dispatch for native clients, sharing the existing application adapter.
use super::{
    access::shared_actor,
    errors::{api_success, ApiError},
    state::HttpApiState,
};
use axum::extract::{Extension, State};
use axum::Json;
use std::sync::Arc;
use talos_app_protocol::requests::{ClientRequest, ClientRequestParams};
use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
use talos_auth::RequestIdentity;

pub(super) async fn dispatch(
    State(state): State<Arc<HttpApiState>>,
    Extension(identity): Extension<RequestIdentity>,
    origin: Option<Extension<talos_app_protocol::relay::RelayCallbackOrigin>>,
    scope: Option<Extension<super::product_routing::SelectedComputer>>,
    Json(mut params): Json<ClientRequestParams>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;
    if let Some(Extension(scope)) = scope {
        params
            .bind_computer_target_from_alias(&scope.computer_id, scope.requested_alias.as_deref())
            .map_err(ApiError::from_talos_error)?;
    }
    if let (Some(origin), ClientRequestParams::McpAuthenticationStart(request)) = (&origin, &params)
    {
        let response = state
            .gateway
            .start_mcp_authentication_with_origin(
                &shared_actor(&identity),
                request.clone(),
                Some(&origin.0 .0),
            )
            .await
            .map_err(ApiError::from_talos_error)?;
        let mut response = api_success(response).into_response();
        response.headers_mut().insert(
            "x-talos-product-operation",
            axum::http::HeaderValue::from_static("mcp-authentication-start"),
        );
        return Ok(response);
    }
    let preview = matches!(params, ClientRequestParams::FilePreviewPrepare(_));
    let method = params.method();
    let adapter = TalosApplicationAdapter::new(
        state.app.clone(),
        state.base_config.agent.timeline_replay_batch_size,
    );
    let response = adapter
        .dispatch(
            &shared_actor(&identity),
            ClientRequest {
                computer_id: None,
                method,
                params,
            },
        )
        .await
        .map_err(|error| ApiError::from_talos_error(error.into()))?;
    let mut response = api_success(response.body).into_response();
    if preview && origin.is_some() {
        response.headers_mut().insert(
            "x-talos-product-operation",
            axum::http::HeaderValue::from_static("file-preview-prepare"),
        );
    }
    Ok(response)
}
