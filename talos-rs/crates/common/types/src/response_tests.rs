use serde::{Deserialize, Serialize};
use serde_json::json;
use talos_error::{ApiError, ErrorCode};

use super::ApiResponse;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
struct SessionCreateResponse {
    session_id: String,
}

#[test]
fn api_response_success_shape_is_tagged_with_data() {
    let response = ApiResponse::success(SessionCreateResponse {
        session_id: "session-1".to_string(),
    });

    let value = serde_json::to_value(response).unwrap();

    assert_eq!(
        value,
        json!({
            "status": "success",
            "data": {
                "session_id": "session-1"
            }
        })
    );
}

#[test]
fn api_response_error_shape_is_tagged_with_error() {
    let response: ApiResponse<SessionCreateResponse> =
        ApiResponse::error(ApiError::new(ErrorCode::SessionNotFound));

    let value = serde_json::to_value(response).unwrap();

    assert_eq!(
        value,
        json!({
            "status": "error",
            "error": {
                "code": "sessionNotFound",
                "message": "Session not found",
                "localization": null
            }
        })
    );
}
