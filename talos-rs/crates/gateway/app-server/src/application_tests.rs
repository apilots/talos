use super::*;
use bytes::Bytes;
use serde::Serialize;
use talos_error::ErrorCode;

#[test]
fn file_upload_base64_accepts_only_canonical_standard_padding() {
    assert_eq!(
        decode_file_upload_data("YWJj", 3).expect("canonical Base64 should decode"),
        Bytes::from_static(b"abc")
    );

    for invalid in ["YWI", "Y WJj", "data:image/png;base64,YWJj"] {
        let error = decode_file_upload_data(invalid, 100)
            .expect_err("non-canonical Base64 should be rejected");
        assert_eq!(
            error,
            ApiError::with_message(
                ErrorCode::InvalidRequest,
                "The file upload is not valid standard Base64."
            )
        );
    }
}

#[test]
fn file_upload_base64_checks_encoded_and_decoded_limits() {
    let encoded_error = decode_file_upload_data("YWJjZA==", 3)
        .expect_err("encoded input should be rejected before decoding");
    assert_eq!(
        encoded_error,
        ApiError::with_message(
            ErrorCode::UploadTooLarge,
            "The encoded file upload exceeds the configured size limit."
        )
    );

    let decoded_error = decode_file_upload_data("YWJj", 2)
        .expect_err("decoded input should be checked after decoding");
    assert_eq!(
        decoded_error,
        ApiError::with_message(
            ErrorCode::UploadTooLarge,
            "The decoded file upload exceeds the configured size limit."
        )
    );
}

#[test]
fn response_serialization_failure_withholds_dependency_details() {
    struct FailedResponse;
    impl Serialize for FailedResponse {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("private-response-sentinel"))
        }
    }
    assert_eq!(
        to_value(FailedResponse)
            .err()
            .expect("serialization must fail"),
        ApiError::with_message(
            ErrorCode::InternalError,
            "Failed to encode the app-server application response."
        )
    );
}
