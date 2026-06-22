use super::*;

#[tokio::test]
async fn test_models_require_bearer_when_auth_enabled() {
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
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
async fn test_models_accept_server_side_cloud_session_cache_when_bearer_missing() {
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
    assert_eq!(resp.status(), StatusCode::OK);
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
async fn test_http_auth_touches_cached_cloud_session_after_bearer_validation() {
    let now_ms = talos_auth::CloudSessionCache::now_ms();
    let old_last_used = now_ms - talos_auth::DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS - 1_000;
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
    cache
        .store(&test_cloud_session_entry("token", old_last_used))
        .expect("cache should be stored");
    let state = Arc::new(
        Arc::try_unwrap(test_state(false))
            .ok()
            .expect("test state should have a single owner")
            .with_cloud_session_cache(cache.clone()),
    );
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(
                    axum::http::header::AUTHORIZATION,
                    format!("Bearer {}", "token"),
                )
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        cache
            .load_candidate(talos_auth::CloudSessionCache::now_ms())
            .expect("cache should load")
            .expect("cache should still exist")
            .last_used_at_ms
            > old_last_used
    );
}

#[tokio::test]
async fn test_http_auth_clears_cached_cloud_session_after_bearer_rejection() {
    let now_ms = talos_auth::CloudSessionCache::now_ms();
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
    cache
        .store(&test_cloud_session_entry("reject", now_ms))
        .expect("cache should be stored");
    let state = Arc::new(
        Arc::try_unwrap(test_state(false))
            .ok()
            .expect("test state should have a single owner")
            .with_cloud_session_cache(cache.clone()),
    );
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header(axum::http::header::AUTHORIZATION, "Bearer reject")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        cache
            .load_candidate(talos_auth::CloudSessionCache::now_ms())
            .expect("cache should load"),
        None
    );
}

#[tokio::test]
async fn test_http_auth_session_store_and_read_require_verified_matching_token() {
    let now_ms = talos_auth::CloudSessionCache::now_ms();
    let client_last_used_at_ms = now_ms + talos_auth::DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS;
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
    let state = Arc::new(
        Arc::try_unwrap(test_state(false))
            .ok()
            .expect("test state should have a single owner")
            .with_cloud_session_cache(cache.clone()),
    );
    let app = Router::new()
        .route(
            "/v1/auth/session",
            get(read_auth_session_handler).post(store_auth_session_handler),
        )
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let store_started_at = talos_auth::CloudSessionCache::now_ms();
    let store_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/v1/auth/session")
                .header(axum::http::header::AUTHORIZATION, "Bearer token")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&talos_types::AuthSessionStoreRequest {
                        session: test_cloud_session_snapshot("token", client_last_used_at_ms),
                    })
                    .expect("body should serialize"),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(store_resp.status(), StatusCode::OK);

    let read_resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/auth/session")
                .header(axum::http::header::AUTHORIZATION, "Bearer token")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(read_resp.status(), StatusCode::OK);
    let stored = cache
        .load_candidate(talos_auth::CloudSessionCache::now_ms())
        .expect("cache should load")
        .expect("cache should exist");
    assert_eq!(stored.access_token, "token");
    assert_ne!(stored.last_used_at_ms, client_last_used_at_ms);
    assert!(stored.last_used_at_ms >= store_started_at);
}

#[tokio::test]
async fn test_http_auth_session_read_does_not_touch_cached_cloud_session() {
    let now_ms = talos_auth::CloudSessionCache::now_ms();
    let old_last_used = now_ms - talos_auth::DEFAULT_CLOUD_SESSION_TOUCH_INTERVAL_MS - 1_000;
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
    cache
        .store(&test_cloud_session_entry("token", old_last_used))
        .expect("cache should be stored");
    let state = Arc::new(
        Arc::try_unwrap(test_state(false))
            .ok()
            .expect("test state should have a single owner")
            .with_cloud_session_cache(cache.clone()),
    );
    let app = Router::new()
        .route("/v1/auth/session", get(read_auth_session_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/auth/session")
                .header(axum::http::header::AUTHORIZATION, "Bearer token")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        cache
            .load_candidate(talos_auth::CloudSessionCache::now_ms())
            .expect("cache should load")
            .expect("cache should exist")
            .last_used_at_ms,
        old_last_used
    );
}

#[tokio::test]
async fn test_http_auth_session_store_rejects_mismatched_token() {
    let now_ms = talos_auth::CloudSessionCache::now_ms();
    let dir = tempfile::tempdir().expect("tempdir should be created");
    let cache = talos_auth::CloudSessionCache::for_talos_home(dir.path());
    let state = Arc::new(
        Arc::try_unwrap(test_state(false))
            .ok()
            .expect("test state should have a single owner")
            .with_cloud_session_cache(cache.clone()),
    );
    let app = Router::new()
        .route("/v1/auth/session", post(store_auth_session_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, auth_middleware));

    let resp = app
        .oneshot(
            Request::builder()
                .method(axum::http::Method::POST)
                .uri("/v1/auth/session")
                .header(axum::http::header::AUTHORIZATION, "Bearer token")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&talos_types::AuthSessionStoreRequest {
                        session: test_cloud_session_snapshot("other-token", now_ms),
                    })
                    .expect("body should serialize"),
                ))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        cache
            .load_candidate(talos_auth::CloudSessionCache::now_ms())
            .expect("cache should load"),
        None
    );
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

fn test_cloud_session_entry(
    token: &str,
    last_used_at_ms: i64,
) -> talos_auth::CloudSessionCacheEntry {
    talos_auth::CloudSessionCacheEntry {
        schema_version: 1,
        cloud_api_base_url: "http://127.0.0.1:8080".to_string(),
        access_token: token.to_string(),
        token_type: "Bearer".to_string(),
        expires_at_ms: Some(
            talos_auth::CloudSessionCache::now_ms()
                + talos_auth::DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS,
        ),
        last_used_at_ms,
        user: talos_auth::CloudSessionUserSnapshot {
            user_id: "user-1".to_string(),
            username: "user-1".to_string(),
            email: Some("user@example.com".to_string()),
            role: talos_auth::UserRole::Admin,
        },
    }
}

fn test_cloud_session_snapshot(
    token: &str,
    last_used_at_ms: i64,
) -> talos_types::AuthSessionSnapshot {
    talos_types::AuthSessionSnapshot {
        schema_version: 1,
        cloud_api_base_url: "http://127.0.0.1:8787".to_string(),
        access_token: token.to_string(),
        token_type: "Bearer".to_string(),
        expires_at_ms: Some(
            (talos_auth::CloudSessionCache::now_ms()
                + talos_auth::DEFAULT_CLOUD_SESSION_IDLE_TIMEOUT_MS)
                .to_string(),
        ),
        last_used_at_ms: last_used_at_ms.to_string(),
        user: talos_types::AuthSessionUserSnapshot {
            user_id: "user-1".to_string(),
            username: "user-1".to_string(),
            email: Some("user@example.com".to_string()),
            role: talos_auth::UserRole::Admin,
        },
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
