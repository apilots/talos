use super::*;
use talos_app_protocol::errors::INVALID_PARAMS;

#[test]
fn file_upload_base64_accepts_only_canonical_standard_padding() {
    assert_eq!(
        decode_file_upload_data("YWJj", 3).expect("canonical Base64 should decode"),
        Bytes::from_static(b"abc")
    );

    for invalid in ["YWI", "Y WJj", "data:image/png;base64,YWJj"] {
        let error = decode_file_upload_data(invalid, 100)
            .expect_err("non-canonical Base64 should be rejected");
        assert_eq!(error.code, INVALID_PARAMS);
    }
}

#[test]
fn file_upload_base64_checks_encoded_and_decoded_limits() {
    let encoded_error = decode_file_upload_data("YWJjZA==", 3)
        .expect_err("encoded input should be rejected before decoding");
    assert_eq!(encoded_error.code, INVALID_PARAMS);
    assert!(encoded_error.message.contains("encoded size limit"));

    let decoded_error = decode_file_upload_data("YWJj", 2)
        .expect_err("decoded input should be checked after decoding");
    assert_eq!(decoded_error.code, INVALID_PARAMS);
    assert!(decoded_error.message.contains("decoded file upload"));
}
