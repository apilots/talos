use super::*;

#[test]
fn localized_failure_has_one_english_and_wire_meaning() {
    let error = ApiError::localized(ErrorLocalization::AttachmentCountExceeded {
        limit: 4,
        actual: 6,
    });
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({
            "code": "attachmentLimitExceeded",
            "message": "This request contains 6 attachments; the limit is 4.",
            "localization": { "type": "attachmentCountExceeded", "limit": 4, "actual": 6 }
        })
    );
    assert_eq!(
        serde_json::from_value::<ApiError>(serde_json::to_value(&error).unwrap()).unwrap(),
        error
    );
    let reworded = ApiError {
        message: "The request has too many attachments: 6 supplied, 4 allowed.".to_string(),
        ..error
    };
    assert_eq!(
        serde_json::from_value::<ApiError>(serde_json::to_value(&reworded).unwrap()).unwrap(),
        reworded
    );
}

#[test]
fn invalid_wire_errors_are_rejected_without_silently_changing_meaning() {
    let valid = serde_json::to_value(ApiError::localized(
        ErrorLocalization::AttachmentCountExceeded {
            limit: 4,
            actual: 6,
        },
    ))
    .unwrap();
    for (pointer, invalid) in [
        ("/code", serde_json::json!("futureCode")),
        ("/code", serde_json::json!("internalError")),
        ("/message", serde_json::json!("")),
        ("/message", serde_json::json!("x".repeat(4097))),
        ("/localization/type", serde_json::json!("futureType")),
        (
            "/localization/actual",
            serde_json::json!(9007199254740992_u64),
        ),
        ("/localization/actual", serde_json::json!(4)),
        ("/localization/limit", serde_json::json!(-1)),
    ] {
        let mut value = valid.clone();
        *value.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            serde_json::from_value::<ApiError>(value.clone()).is_err(),
            "accepted {value}"
        );
    }
    let mut missing = valid;
    missing.as_object_mut().unwrap().remove("localization");
    assert!(serde_json::from_value::<ApiError>(missing).is_err());
}

#[test]
fn oversized_descriptor_keeps_original_failure_with_explicit_bounded_text() {
    let error = ApiError::localized(ErrorLocalization::ModelNotConfigured {
        provider: "p".to_string(),
        model: "a".repeat(5000),
    });
    assert_eq!(error.code, ErrorCode::ProviderUnavailable);
    assert_eq!(error.localization, None);
    assert!(error.message.ends_with(" [truncated]"));
    assert_eq!(error.message.chars().count(), 4096);
}
