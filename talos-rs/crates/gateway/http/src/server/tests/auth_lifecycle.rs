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
async fn test_models_require_bearer_even_when_local_cache_exists() {
    let now_ms = talos_auth::CloudSessionCache::now_ms();
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
    cache
        .store(&test_cloud_session_entry("token", now_ms))
        .expect("cache should be stored");
    let state = Arc::new(
        Arc::try_unwrap(test_state(false))
            .ok()
            .expect("test state should have a single owner")
            .with_cloud_session_cache(cache),
    );
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
async fn test_unknown_auth_route_is_not_exposed() {
    let state = test_state(false);
    let app = Router::new()
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/auth/unknown")
                .header(axum::http::header::AUTHORIZATION, "Bearer token")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

fn test_cloud_session_entry(
    token: &str,
    last_used_at_ms: i64,
) -> talos_auth::CloudSessionCacheEntry {
    let user = talos_auth::UserDto {
        user_id: "user-1".to_string(),
        username: "user-1".to_string(),
        email: Some("user@example.com".to_string()),
        role: talos_auth::UserRole::User,
        r#type: talos_auth::UserType::Free,
        disabled: false,
        created_at: 0,
        updated_at: 0,
    };
    let mut entry = talos_auth::CloudSessionCacheEntry::from_verified_user(
        "http://127.0.0.1:8080",
        token,
        "Bearer",
        Some(
            talos_auth::CloudSessionCache::now_ms()
                + talos_auth::DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS,
        ),
        &user,
        talos_auth::CloudSessionCache::now_ms(),
    );
    entry.last_used_at_ms = last_used_at_ms;
    entry
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
    assert!(payload["sessions"].is_array());
}

#[tokio::test]
async fn test_get_llm_allows_auth_disabled_actor() {
    let state = test_state(true);
    let response = get_llm_handler(State(state), None)
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
