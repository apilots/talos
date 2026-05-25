use super::*;

#[tokio::test]
async fn test_models_require_bearer_when_auth_enabled() {
    let state = test_state(false);
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_models_allow_no_token_when_auth_disabled() {
    let state = test_state(true);
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_removed_local_auth_paths_do_not_require_bearer() {
    let state = test_state(false);
    let app = Router::new()
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    for uri in ["/v1/auth/register", "/v1/auth/login"] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(axum::http::Method::POST)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
async fn test_list_sessions_allows_anonymous_route_when_auth_disabled() {
    let state = test_state(true);
    let app = Router::new()
        .route("/v1/sessions", get(list_sessions_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/sessions")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert!(payload["data"].is_array());
}

#[tokio::test]
async fn test_get_llm_allows_auth_disabled_actor() {
    let state = test_state(true);
    let response = get_llm_handler(State(state))
        .await
        .expect("auth disabled actor should access llm config");
    assert_eq!(response.0.config.provider, "anthropic");
}

#[tokio::test]
async fn test_lifecycle_gate_rejects_new_requests_when_quiescing() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    lifecycle.store(ServerLifecycleState::Quiescing);
    let llm_svc: Arc<dyn talos_config::LlmConfigService> =
        Arc::new(talos_config::LlmConfigManager::new(Arc::new(
            talos_config::InMemoryLlmConfigStore::new(Default::default()),
        )));
    let state = test_state_with_gateway_event_store_config_and_lifecycle(
        Arc::new(MockGateway),
        true,
        Arc::new(RecordingEventStore::default()),
        test_config(),
        lifecycle,
        llm_svc,
    );
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            lifecycle_gate_middleware,
        ))
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_lifecycle_gate_allows_health_when_quiescing() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    lifecycle.store(ServerLifecycleState::Quiescing);
    let llm_svc: Arc<dyn talos_config::LlmConfigService> =
        Arc::new(talos_config::LlmConfigManager::new(Arc::new(
            talos_config::InMemoryLlmConfigStore::new(Default::default()),
        )));
    let state = test_state_with_gateway_event_store_config_and_lifecycle(
        Arc::new(MockGateway),
        true,
        Arc::new(RecordingEventStore::default()),
        test_config(),
        lifecycle,
        llm_svc,
    );
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            lifecycle_gate_middleware,
        ))
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::OK);
}
