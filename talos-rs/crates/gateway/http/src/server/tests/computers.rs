use super::*;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, headers, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn computer_http_management_preserves_identity_and_redacts_directory_secrets() {
    let state = test_state(false).await;
    // Exercise the actual Computer routes and authentication middleware with real node storage.
    let app = super::super::computers::routes()
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware));
    let (status, _, _) = call(
        &app,
        "GET",
        "/v1/computers",
        /* token */ None,
        serde_json::json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, headers, created) = call(
        &app,
        "POST",
        "/v1/computers",
        Some("owner"),
        serde_json::json!({
            "displayName": "Laptop", "credentialExpiresAt": chrono::Utc::now().timestamp() + 3600
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    let node_id = created["data"]["node"]["nodeId"].as_str().unwrap();
    let credential = created["data"]["credential"]["token"].as_str().unwrap();
    assert!(!node_id.is_empty());
    assert!(!credential.is_empty());
    let path = format!("/v1/computers/{node_id}");
    let (status, _, _) = call(&app, "GET", &path, Some("other"), serde_json::json!(null)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, page) = call(
        &app,
        "GET",
        "/v1/computers?query=laptop&limit=1",
        Some("owner"),
        serde_json::json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["data"]["total"], 1);
    assert_eq!(page["data"]["data"][0]["nodeId"], node_id);
    assert!(!page.to_string().contains(credential));
    assert_eq!(page["data"]["data"][0]["available"], false);
    let (status, _, renamed) = call(
        &app,
        "PATCH",
        &format!("{path}/name"),
        Some("owner"),
        serde_json::json!({
            "nodeId": node_id, "displayName": "Renamed laptop"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["data"]["nodeId"], node_id);
    let (status, _, disabled) = call(
        &app,
        "PUT",
        &format!("{path}/status"),
        Some("owner"),
        serde_json::json!({
            "nodeId": node_id, "status": "disabled"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled["data"]["displayName"], "Renamed laptop");
    assert_eq!(disabled["data"]["status"], "disabled");
    let (status, headers, rotated) = call(
        &app,
        "POST",
        &format!("{path}/credential"),
        Some("owner"),
        serde_json::json!({
            "nodeId": node_id, "expiresAt": chrono::Utc::now().timestamp() + 7200
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(rotated["data"]["nodeId"], node_id);
    assert_ne!(rotated["data"]["token"], credential);
    let (status, _, read) = call(&app, "GET", &path, Some("owner"), serde_json::json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["data"]["nodeId"], node_id);
    assert!(!read.to_string().contains(credential));
    assert!(!read
        .to_string()
        .contains(rotated["data"]["token"].as_str().unwrap()));
}
