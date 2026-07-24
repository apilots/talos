use super::*;

#[test]
fn maps_common_not_found_error() {
    let body = AppServerErrorBody::from_talos_error(TalosError::ResourceNotFound(
        "session s1".to_string(),
    ));
    assert_eq!(body.code, NOT_FOUND);
    assert_eq!(body.message, "resource not found: session s1");
}

#[test]
fn maps_every_file_error_kind_to_validation_failed() {
    for kind in [
        talos_error::TalosFileErrorKind::UploadTooLarge,
        talos_error::TalosFileErrorKind::UnsupportedMediaType,
        talos_error::TalosFileErrorKind::AttachmentLimitExceeded,
        talos_error::TalosFileErrorKind::UnsupportedAttachment,
    ] {
        let body = AppServerErrorBody::from_talos_error(TalosError::FileError {
            kind,
            message: "file input rejected".to_string(),
        });
        assert_eq!(body.code, VALIDATION_FAILED);
    }
}
