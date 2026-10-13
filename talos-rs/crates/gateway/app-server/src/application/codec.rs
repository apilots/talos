//! Application response encoding and bounded upload decoding.
use super::{ApplicationResponse, ApplicationResult};
use base64::Engine;
use bytes::Bytes;
use serde::Serialize;
use talos_error::{ApiError, ErrorCode, TalosError};

pub(super) fn to_value<T: Serialize>(value: T) -> ApplicationResult<ApplicationResponse> {
    serde_json::to_value(value)
        .map(|body| ApplicationResponse {
            body,
            started_turn: None,
        })
        .map_err(|error| {
            talos_error_to_api_error(TalosError::internal_with_source(
                "Failed to encode the app-server application response.",
                error,
            ))
        })
}

pub(super) fn decode_file_upload_data(
    data_base64: &str,
    max_upload_bytes: u64,
) -> ApplicationResult<Bytes> {
    let max_encoded_len = max_upload_bytes
        .saturating_add(2)
        .saturating_div(3)
        .saturating_mul(4);
    let encoded_len = u64::try_from(data_base64.len()).map_err(|error| {
        talos_error_to_api_error(TalosError::from_source(
            ErrorCode::UploadTooLarge,
            "The encoded file upload exceeds the configured size limit.",
            error,
        ))
    })?;
    if encoded_len > max_encoded_len {
        return Err(talos_error_to_api_error(TalosError::new(
            ErrorCode::UploadTooLarge,
            "The encoded file upload exceeds the configured size limit.",
        )));
    }

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .map_err(|error| {
            talos_error_to_api_error(TalosError::from_source(
                ErrorCode::InvalidRequest,
                "The file upload is not valid standard Base64.",
                error,
            ))
        })?;
    if decoded.len() as u64 > max_upload_bytes {
        return Err(talos_error_to_api_error(TalosError::new(
            ErrorCode::UploadTooLarge,
            "The decoded file upload exceeds the configured size limit.",
        )));
    }
    Ok(Bytes::from(decoded))
}

pub(super) fn talos_error_to_api_error(error: TalosError) -> ApiError {
    talos_logging::report_error(&error);
    error.to_api_error()
}
