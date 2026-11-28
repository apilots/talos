use super::*;
use crate::{ProductRequestRouter, ProductRoute, ProductRouteFuture};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Routes(AtomicUsize);
impl ProductRequestRouter for Routes {
    fn route<'a>(
        &'a self,
        actor: talos_auth::Actor,
        computer: String,
        request: axum::extract::Request,
    ) -> ProductRouteFuture<'a> {
        Box::pin(async move {
            assert_eq!(actor.user_id, "owner");
            assert_eq!(computer, "personal");
            assert!(!request.headers().contains_key("x-talos-computer"));
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(ProductRoute::Remote(
                (StatusCode::OK, "personal product").into_response(),
            ))
        })
    }
}
async fn routing_app(router: Option<Arc<Routes>>) -> Router {
    let mut state = (*test_state(false).await).clone();
    state.product_router = router.map(|router| router as Arc<dyn ProductRequestRouter>);
    let state = Arc::new(state);
    Router::new()
        .route("/v1/sessions", get(|| async { "local product" }))
        .route("/v1/computers", get(|| async { "central directory" }))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            super::super::product_routing::route_product_request,
        ))
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware))
}
async fn selected(app: Router, path: &str, token: bool) -> axum::response::Response {
    let mut r = Request::builder()
        .uri(path)
        .header("x-talos-computer", "personal");
    if token {
        r = r.header("authorization", "Bearer owner");
    }
    app.oneshot(r.body(Body::empty()).unwrap()).await.unwrap()
}
#[tokio::test]
async fn personal_product_routing_requires_authenticated_actor() {
    let router = Arc::new(Routes(AtomicUsize::new(0)));
    assert_eq!(
        selected(
            routing_app(Some(router.clone())).await,
            "/v1/sessions",
            false
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(router.0.load(Ordering::SeqCst), 0);
    let response = selected(
        routing_app(Some(router.clone())).await,
        "/v1/sessions",
        true,
    )
    .await;
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap(),
        "personal product"
    );
    assert_eq!(router.0.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn central_directory_never_enters_personal_relay() {
    let router = Arc::new(Routes(AtomicUsize::new(0)));
    let response = selected(
        routing_app(Some(router.clone())).await,
        "/v1/computers",
        true,
    )
    .await;
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap(),
        "central directory"
    );
    assert_eq!(router.0.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn unassembled_routing_is_explicitly_unavailable() {
    assert_eq!(
        selected(routing_app(None).await, "/v1/sessions", true)
            .await
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}
