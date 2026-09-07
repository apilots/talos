use super::errors::ApiError;
use axum::response::IntoResponse;
use talos_error::{ErrorCode, ErrorLocalization, TalosError};

#[tokio::test]
async fn framework_route_and_unclassified_failures_use_the_shared_contract() {
    use axum::{body::Body, http::Request, middleware, routing::get, Router};
    use tower::ServiceExt;

    let app = Router::new()
        .route(
            "/v1/failure",
            get(|| async {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    [("retry-after", "30")],
                    "private-framework-sentinel",
                )
            }),
        )
        .layer(middleware::from_fn(
            super::errors::normalize_extractor_rejection,
        ));
    for (method, path, status, code, message) in [
        (
            "GET",
            "/v1/missing",
            404,
            ErrorCode::ResourceNotFound,
            "The requested API endpoint was not found.",
        ),
        (
            "POST",
            "/v1/failure",
            405,
            ErrorCode::InvalidRequest,
            "The HTTP method is not supported for this endpoint.",
        ),
        (
            "GET",
            "/v1/failure",
            503,
            ErrorCode::InternalError,
            "The API request failed unexpectedly; technical details were withheld.",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        if status == 405 {
            assert_eq!(response.headers()["allow"], "GET,HEAD");
        }
        if status == 503 {
            assert_eq!(response.headers()["retry-after"], "30");
        }
        let bytes = axum::body::to_bytes(response.into_body(), 16_384)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::to_value(talos_types::ApiResponse::<()>::error(
                talos_error::ApiError::with_message(code, message)
            ))
            .unwrap()
        );
    }
}

#[tokio::test]
async fn response_preserves_specific_message_and_descriptor_without_diagnostics() {
    for error in [
        TalosError::localized(ErrorLocalization::ModelNotConfigured {
            provider: "test-provider".to_string(),
            model: "missing-model".to_string(),
        })
        .context("private-config-path-sentinel"),
        TalosError::internal_with_source(
            "Failed to load session: database is locked.",
            std::io::Error::other("private-database-password-sentinel"),
        ),
    ] {
        let expected = talos_types::ApiResponse::<()>::error(error.to_api_error());
        let response = ApiError::from_talos_error(error).into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 16_384)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert!(!String::from_utf8(bytes.to_vec())
            .unwrap()
            .contains("sentinel"));
    }
}

#[tokio::test]
async fn two_business_causes_with_the_same_code_remain_distinct() {
    let messages = [
        "The configured timeout must be greater than zero.",
        "A session title must not be empty.",
    ];
    for message in messages {
        let response =
            ApiError::from_talos_error(TalosError::new(ErrorCode::InvalidRequest, message))
                .into_response();
        let bytes = axum::body::to_bytes(response.into_body(), 16_384)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["message"], message);
    }
}
