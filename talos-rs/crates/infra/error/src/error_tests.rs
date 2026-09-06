use super::{ApiError, ErrorCode, TalosError};
use std::error::Error;

#[test]
fn context_and_clone_keep_original_source_and_capture() {
    let original = TalosError::from_source(
        ErrorCode::PermissionDenied,
        "read session",
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied"),
    );
    let contextual = original.clone().context("resume turn");
    assert!(std::ptr::eq(original.backtrace(), contextual.backtrace()));
    assert!(std::ptr::eq(
        original.source().unwrap(),
        contextual.source().unwrap()
    ));
    assert_eq!(contextual.location(), original.location());
    assert_eq!(contextual.to_api_error(), original.to_api_error());
    assert_eq!(contextual.diagnostic_message(), "resume turn: read session");
    assert_ne!(
        contextual.backtrace().status(),
        std::backtrace::BacktraceStatus::Disabled
    );
}

#[test]
fn anyhow_context_enters_internal_without_stringifying_its_chain() {
    let cause = anyhow::Error::new(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "access denied",
    ))
    .context("read configuration");
    let error = TalosError::internal_with_source("start service", cause);
    let source = error.source().unwrap();
    assert_eq!(source.to_string(), "read configuration");
    assert_eq!(source.source().unwrap().to_string(), "access denied");
}

#[test]
fn api_error_uses_code_default_message() {
    assert_eq!(
        ApiError::new(ErrorCode::ResourceNotFound),
        ApiError {
            code: ErrorCode::ResourceNotFound,
            message: "Resource not found".to_string(),
            localization: None,
        }
    );
}

#[test]
fn error_code_serializes_as_camel_case() {
    let value = serde_json::to_string(&ErrorCode::ActiveTurnRunning).unwrap();
    assert_eq!(value, "\"activeTurnRunning\"");
    let decoded: ErrorCode = serde_json::from_str(&value).unwrap();
    assert_eq!(decoded, ErrorCode::ActiveTurnRunning);
}

#[test]
fn message_queue_full_has_stable_wire_value_and_message() {
    let value = serde_json::to_string(&ErrorCode::MessageQueueFull).unwrap();

    assert_eq!(value, "\"messageQueueFull\"");
    assert_eq!(
        ApiError::new(ErrorCode::MessageQueueFull),
        ApiError {
            code: ErrorCode::MessageQueueFull,
            message: "Message queue is full".to_string(),
            localization: None,
        }
    );
}

#[test]
fn message_queue_busy_has_stable_wire_value_and_message() {
    let value = serde_json::to_string(&ErrorCode::MessageQueueBusy).unwrap();

    assert_eq!(value, "\"messageQueueBusy\"");
    assert_eq!(
        ApiError::new(ErrorCode::MessageQueueBusy),
        ApiError {
            code: ErrorCode::MessageQueueBusy,
            message: "Message queue message is busy".to_string(),
            localization: None,
        }
    );
}

#[test]
fn unregistered_error_code_fails_to_decode() {
    let error = serde_json::from_str::<ErrorCode>("\"futureCode\"").unwrap_err();
    assert!(error.is_data());
}

#[test]
fn business_and_internal_explanations_are_not_replaced_by_code_defaults() {
    assert_eq!(
        TalosError::new(ErrorCode::InvalidRequest, "bad request detail").to_api_error(),
        ApiError::with_message(ErrorCode::InvalidRequest, "bad request detail")
    );
    assert_eq!(
        TalosError::internal("Failed to load session: database is locked.").to_api_error(),
        ApiError::with_message(
            ErrorCode::InternalError,
            "Failed to load session: database is locked."
        )
    );
}

#[test]
fn talos_error_keeps_internal_diagnostics_out_of_public_error() {
    let error = TalosError::internal_with_source(
        "Failed to read configuration; technical details were withheld.",
        std::io::Error::other("secret path leaked"),
    )
    .context("private operation context");

    assert!(error.is_internal());
    assert!(error
        .diagnostic_message()
        .contains("private operation context"));
    assert_eq!(error.source().unwrap().to_string(), "secret path leaked");
    assert_eq!(
        error.to_api_error(),
        ApiError::with_message(
            ErrorCode::InternalError,
            "Failed to read configuration; technical details were withheld."
        )
    );
}
