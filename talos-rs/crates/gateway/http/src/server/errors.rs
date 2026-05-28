use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(super) struct ApiErrorBody {
    code: String,
    message: String,
}

#[derive(Debug)]
pub(super) struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

impl ApiError {
    pub(super) fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code: code.into(),
                message: message.into(),
            },
        }
    }

    pub(super) fn from_talos_error(error: talos_error::TalosError) -> Self {
        match error {
            talos_error::TalosError::AuthError(message) => {
                Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
            }
            talos_error::TalosError::ValidationError(message) => {
                Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
            }
            talos_error::TalosError::ResourceNotFound(message) => {
                Self::new(StatusCode::NOT_FOUND, "not_found", message)
            }
            other => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                other.to_string(),
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
