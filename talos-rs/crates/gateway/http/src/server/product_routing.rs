//! Optional target routing after authentication, before product handlers.
use super::{access::shared_actor, errors::ApiError, state::HttpApiState};
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{future::Future, pin::Pin, sync::Arc};
use talos_auth::RequestIdentity;
use talos_error::{ErrorCode, TalosError, TalosResult};

/// Authenticated product scope resolved before typed business dispatch.
#[derive(Clone)]
pub struct SelectedComputer {
    pub computer_id: String,
    pub requested_alias: Option<String>,
}

pub enum ProductRoute {
    Local(Request),
    Remote(Response),
}

pub type ProductRouteFuture<'a> =
    Pin<Box<dyn Future<Output = TalosResult<ProductRoute>> + Send + 'a>>;

/// Host composition supplies account-authorized target routing. Implementations must
/// keep personal content transient and preserve request cancellation/backpressure.
pub trait ProductRequestRouter: Send + Sync {
    /// Resolve an original synchronized source identity through current owned
    /// product registration. This lookup must never open a Sync connection.
    fn resolve_source_peer<'a>(
        &'a self,
        _actor: talos_auth::Actor,
        _peer_id: String,
    ) -> Pin<Box<dyn Future<Output = TalosResult<String>> + Send + 'a>> {
        Box::pin(async {
            Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Source product routing is not assembled.",
            ))
        })
    }

    /// Resource-origin callers possess only the Host's opaque preview lease.
    /// Implementations must restrict this path to read-only preview delivery.
    fn preview<'a>(&'a self, _computer_id: String, _request: Request) -> ProductRouteFuture<'a> {
        Box::pin(async {
            Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Personal preview relay is not assembled.",
            ))
        })
    }
    fn route<'a>(
        &'a self,
        actor: talos_auth::Actor,
        computer_id: String,
        request: Request,
    ) -> ProductRouteFuture<'a>;
}

pub(super) async fn route_product_request(
    State(state): State<Arc<HttpApiState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let target = request.headers_mut().remove("x-talos-computer");
    let peer = request.headers_mut().remove("x-talos-source-peer");
    if target.is_none() && peer.is_none() {
        return next.run(request).await;
    }
    if target.is_some() && peer.is_some() {
        return ApiError::from_talos_error(TalosError::new(
            ErrorCode::InvalidRequest,
            "Only one product routing scope is allowed.",
        ))
        .into_response();
    }
    let header = target
        .as_ref()
        .or(peer.as_ref())
        .expect("Product routing scope");
    let selected = match header.to_str() {
        Ok(target) if !target.trim().is_empty() && target.len() <= 128 => target.to_owned(),
        _ => {
            return ApiError::from_talos_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "Product routing identity is invalid.",
            ))
            .into_response()
        }
    };
    let target = if peer.is_some() {
        let Some(identity) = request.extensions().get::<RequestIdentity>() else {
            return ApiError::new(ErrorCode::Unauthenticated).into_response();
        };
        let Some(router) = &state.product_router else {
            return ApiError::from_talos_error(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Source product routing is not assembled.",
            ))
            .into_response();
        };
        match router
            .resolve_source_peer(shared_actor(identity), selected)
            .await
        {
            Ok(computer) => computer,
            Err(error) => return ApiError::from_talos_error(error).into_response(),
        }
    } else {
        selected
    };
    if central_route(request.uri().path()) || target == "embedded" {
        return next.run(request).await;
    }
    let Some(identity) = request.extensions().get::<RequestIdentity>() else {
        return ApiError::new(ErrorCode::Unauthenticated).into_response();
    };
    let actor = shared_actor(identity);
    let Some(router) = &state.product_router else {
        return ApiError::from_talos_error(TalosError::new(
            ErrorCode::ServiceUnavailable,
            "Remote product routing is not assembled in this product",
        ))
        .into_response();
    };
    request.extensions_mut().insert(SelectedComputer {
        computer_id: target.clone(),
        requested_alias: None,
    });
    match router.route(actor, target, request).await {
        Ok(ProductRoute::Local(request)) => next.run(request).await,
        Ok(ProductRoute::Remote(response)) => response,
        Err(error) => ApiError::from_talos_error(error).into_response(),
    }
}

fn central_route(path: &str) -> bool {
    path == "/health"
        || path == "/v1/user"
        || path.starts_with("/v1/auth/")
        || path.starts_with("/v1/computers")
        || path.starts_with("/v1/worker-runtime")
        || path.starts_with("/v1/desktop/")
        || !path.starts_with("/v1/")
}
