//! Projection of the existing Talos error contract to Agent protobuf failures.

use agent_pb as pb;
use talos_error::{ApiError, ErrorCode, ErrorLocalization, TalosError, TalosResult};

pub fn encode_failure(error: &TalosError) -> pb::Failure {
    pb::Failure {
        error: Some(encode_api_error(error.to_api_error())),
    }
}

pub fn encode_api_error(api: ApiError) -> pb::ApiError {
    let localization = api.localization.map(|value| match value {
        ErrorLocalization::ComputerDeletionActive => {
            pb::api_error::Localization::ComputerDeletionActive(pb::Empty {})
        }
        ErrorLocalization::ComputerRegistrationRemoved => {
            pb::api_error::Localization::ComputerRegistrationRemoved(pb::Empty {})
        }
        ErrorLocalization::ComputerDeletionUnverified => {
            pb::api_error::Localization::ComputerDeletionUnverified(pb::Empty {})
        }
        ErrorLocalization::ModelNotConfigured { provider, model } => {
            pb::api_error::Localization::ModelNotConfigured(pb::ModelNotConfigured {
                provider,
                model,
            })
        }
        ErrorLocalization::AttachmentCountExceeded { limit, actual } => {
            pb::api_error::Localization::AttachmentCountExceeded(pb::AttachmentCountExceeded {
                limit,
                actual,
            })
        }
    });
    pb::ApiError {
        code: encode_code(api.code) as i32,
        message: api.message,
        localization,
    }
}

pub fn decode_failure(failure: pb::Failure) -> TalosResult<TalosError> {
    decode_api_error(failure.error.ok_or_else(malformed_failure)?).map(TalosError::from)
}

pub fn decode_api_error(error: pb::ApiError) -> TalosResult<ApiError> {
    let code = decode_code(error.code)?;
    let localization = error.localization.map(|value| match value {
        pb::api_error::Localization::ComputerDeletionActive(_) => {
            ErrorLocalization::ComputerDeletionActive
        }
        pb::api_error::Localization::ComputerRegistrationRemoved(_) => {
            ErrorLocalization::ComputerRegistrationRemoved
        }
        pb::api_error::Localization::ComputerDeletionUnverified(_) => {
            ErrorLocalization::ComputerDeletionUnverified
        }
        pb::api_error::Localization::ModelNotConfigured(value) => {
            ErrorLocalization::ModelNotConfigured {
                provider: value.provider,
                model: value.model,
            }
        }
        pb::api_error::Localization::AttachmentCountExceeded(value) => {
            ErrorLocalization::AttachmentCountExceeded {
                limit: value.limit,
                actual: value.actual,
            }
        }
    });
    ApiError::from_wire_parts(code, error.message, localization)
}

/// Typed business failures use protobuf details, including pre-admission failures.
pub fn encode_status(error: &TalosError) -> tonic::Status {
    use prost::Message;
    let code = match error.error_code() {
        ErrorCode::Unauthenticated | ErrorCode::ResourceExpired => tonic::Code::Unauthenticated,
        ErrorCode::PermissionDenied => tonic::Code::PermissionDenied,
        ErrorCode::InvalidRequest | ErrorCode::InvalidPath => tonic::Code::InvalidArgument,
        ErrorCode::ResourceNotFound | ErrorCode::SessionNotFound => tonic::Code::NotFound,
        ErrorCode::Timeout => tonic::Code::DeadlineExceeded,
        ErrorCode::Cancelled => tonic::Code::Cancelled,
        ErrorCode::ServiceUnavailable => tonic::Code::Unavailable,
        _ => tonic::Code::Internal,
    };
    tonic::Status::with_details(
        code,
        error.to_api_error().message,
        encode_failure(error).encode_to_vec().into(),
    )
}

pub fn decode_status(status: tonic::Status) -> TalosError {
    use prost::Message;
    if !status.details().is_empty() {
        return pb::Failure::decode(status.details())
            .map_err(|_| malformed_failure())
            .and_then(decode_failure)
            .unwrap_or_else(|error| error)
            .with_source(status);
    }
    // Tonic classifies its local deadline error as Cancelled. Preserve the
    // typed timeout cause without treating a remote cancellation as timeout.
    let mut source = std::error::Error::source(&status);
    for _ in 0..16 {
        let Some(error) = source else { break };
        if error.is::<tonic::TimeoutExpired>() {
            return TalosError::from_source(
                ErrorCode::Timeout,
                "Agent RPC response timed out",
                status,
            );
        }
        source = error.source();
    }
    let code = match status.code() {
        tonic::Code::DeadlineExceeded => ErrorCode::Timeout,
        tonic::Code::Cancelled => ErrorCode::Cancelled,
        tonic::Code::Unavailable => ErrorCode::ServiceUnavailable,
        tonic::Code::Unauthenticated => ErrorCode::Unauthenticated,
        tonic::Code::PermissionDenied => ErrorCode::PermissionDenied,
        tonic::Code::InvalidArgument => ErrorCode::InvalidRequest,
        tonic::Code::NotFound => ErrorCode::ResourceNotFound,
        _ => ErrorCode::InternalError,
    };
    TalosError::from_source(code, "Agent RPC did not complete", status)
}

fn encode_code(code: ErrorCode) -> pb::ErrorCode {
    match code {
        ErrorCode::InvalidRequest => pb::ErrorCode::InvalidRequest,
        ErrorCode::InvalidRetryRequest => pb::ErrorCode::InvalidRetryRequest,
        ErrorCode::Unauthenticated => pb::ErrorCode::Unauthenticated,
        ErrorCode::PermissionDenied => pb::ErrorCode::PermissionDenied,
        ErrorCode::ResourceNotFound => pb::ErrorCode::ResourceNotFound,
        ErrorCode::SessionNotFound => pb::ErrorCode::SessionNotFound,
        ErrorCode::ResourceExpired => pb::ErrorCode::ResourceExpired,
        ErrorCode::Conflict => pb::ErrorCode::Conflict,
        ErrorCode::ActiveTurnRunning => pb::ErrorCode::ActiveTurnRunning,
        ErrorCode::MessageQueueFull => pb::ErrorCode::MessageQueueFull,
        ErrorCode::MessageQueueBusy => pb::ErrorCode::MessageQueueBusy,
        ErrorCode::RetrySourceCompleted => pb::ErrorCode::RetrySourceCompleted,
        ErrorCode::RetrySourceMismatch => pb::ErrorCode::RetrySourceMismatch,
        ErrorCode::IdempotencyConflict => pb::ErrorCode::IdempotencyConflict,
        ErrorCode::InvalidCursor => pb::ErrorCode::InvalidCursor,
        ErrorCode::EventCursorExpired => pb::ErrorCode::EventCursorExpired,
        ErrorCode::FsDisabled => pb::ErrorCode::FsDisabled,
        ErrorCode::InvalidPath => pb::ErrorCode::InvalidPath,
        ErrorCode::PathOutsideRoot => pb::ErrorCode::PathOutsideRoot,
        ErrorCode::SymlinkNotSupported => pb::ErrorCode::SymlinkNotSupported,
        ErrorCode::PathNotFound => pb::ErrorCode::PathNotFound,
        ErrorCode::NotDirectory => pb::ErrorCode::NotDirectory,
        ErrorCode::InvalidName => pb::ErrorCode::InvalidName,
        ErrorCode::AlreadyExists => pb::ErrorCode::AlreadyExists,
        ErrorCode::UploadTooLarge => pb::ErrorCode::UploadTooLarge,
        ErrorCode::UnsupportedMediaType => pb::ErrorCode::UnsupportedMediaType,
        ErrorCode::AttachmentLimitExceeded => pb::ErrorCode::AttachmentLimitExceeded,
        ErrorCode::UnsupportedAttachment => pb::ErrorCode::UnsupportedAttachment,
        ErrorCode::FileInputRejected => pb::ErrorCode::FileInputRejected,
        ErrorCode::ContextWindowExceeded => pb::ErrorCode::ContextWindowExceeded,
        ErrorCode::SessionBudgetExceeded => pb::ErrorCode::SessionBudgetExceeded,
        ErrorCode::UsageLimitExceeded => pb::ErrorCode::UsageLimitExceeded,
        ErrorCode::ContextCompactionFailed => pb::ErrorCode::ContextCompactionFailed,
        ErrorCode::ProviderUnavailable => pb::ErrorCode::ProviderUnavailable,
        ErrorCode::ToolExecutionFailed => pb::ErrorCode::ToolExecutionFailed,
        ErrorCode::Timeout => pb::ErrorCode::Timeout,
        ErrorCode::Cancelled => pb::ErrorCode::Cancelled,
        ErrorCode::ServiceUnavailable => pb::ErrorCode::ServiceUnavailable,
        ErrorCode::RunForcedAbort => pb::ErrorCode::RunForcedAbort,
        ErrorCode::RunCleanupUnconfirmed => pb::ErrorCode::RunCleanupUnconfirmed,
        ErrorCode::InternalError => pb::ErrorCode::InternalError,
    }
}

fn decode_code(value: i32) -> TalosResult<ErrorCode> {
    let code = match pb::ErrorCode::try_from(value).map_err(|_| malformed_failure())? {
        pb::ErrorCode::Unspecified => return Err(malformed_failure()),
        pb::ErrorCode::InvalidRequest => ErrorCode::InvalidRequest,
        pb::ErrorCode::InvalidRetryRequest => ErrorCode::InvalidRetryRequest,
        pb::ErrorCode::Unauthenticated => ErrorCode::Unauthenticated,
        pb::ErrorCode::PermissionDenied => ErrorCode::PermissionDenied,
        pb::ErrorCode::ResourceNotFound => ErrorCode::ResourceNotFound,
        pb::ErrorCode::SessionNotFound => ErrorCode::SessionNotFound,
        pb::ErrorCode::ResourceExpired => ErrorCode::ResourceExpired,
        pb::ErrorCode::Conflict => ErrorCode::Conflict,
        pb::ErrorCode::ActiveTurnRunning => ErrorCode::ActiveTurnRunning,
        pb::ErrorCode::MessageQueueFull => ErrorCode::MessageQueueFull,
        pb::ErrorCode::MessageQueueBusy => ErrorCode::MessageQueueBusy,
        pb::ErrorCode::RetrySourceCompleted => ErrorCode::RetrySourceCompleted,
        pb::ErrorCode::RetrySourceMismatch => ErrorCode::RetrySourceMismatch,
        pb::ErrorCode::IdempotencyConflict => ErrorCode::IdempotencyConflict,
        pb::ErrorCode::InvalidCursor => ErrorCode::InvalidCursor,
        pb::ErrorCode::EventCursorExpired => ErrorCode::EventCursorExpired,
        pb::ErrorCode::FsDisabled => ErrorCode::FsDisabled,
        pb::ErrorCode::InvalidPath => ErrorCode::InvalidPath,
        pb::ErrorCode::PathOutsideRoot => ErrorCode::PathOutsideRoot,
        pb::ErrorCode::SymlinkNotSupported => ErrorCode::SymlinkNotSupported,
        pb::ErrorCode::PathNotFound => ErrorCode::PathNotFound,
        pb::ErrorCode::NotDirectory => ErrorCode::NotDirectory,
        pb::ErrorCode::InvalidName => ErrorCode::InvalidName,
        pb::ErrorCode::AlreadyExists => ErrorCode::AlreadyExists,
        pb::ErrorCode::UploadTooLarge => ErrorCode::UploadTooLarge,
        pb::ErrorCode::UnsupportedMediaType => ErrorCode::UnsupportedMediaType,
        pb::ErrorCode::AttachmentLimitExceeded => ErrorCode::AttachmentLimitExceeded,
        pb::ErrorCode::UnsupportedAttachment => ErrorCode::UnsupportedAttachment,
        pb::ErrorCode::FileInputRejected => ErrorCode::FileInputRejected,
        pb::ErrorCode::ContextWindowExceeded => ErrorCode::ContextWindowExceeded,
        pb::ErrorCode::SessionBudgetExceeded => ErrorCode::SessionBudgetExceeded,
        pb::ErrorCode::UsageLimitExceeded => ErrorCode::UsageLimitExceeded,
        pb::ErrorCode::ContextCompactionFailed => ErrorCode::ContextCompactionFailed,
        pb::ErrorCode::ProviderUnavailable => ErrorCode::ProviderUnavailable,
        pb::ErrorCode::ToolExecutionFailed => ErrorCode::ToolExecutionFailed,
        pb::ErrorCode::Timeout => ErrorCode::Timeout,
        pb::ErrorCode::Cancelled => ErrorCode::Cancelled,
        pb::ErrorCode::ServiceUnavailable => ErrorCode::ServiceUnavailable,
        pb::ErrorCode::RunForcedAbort => ErrorCode::RunForcedAbort,
        pb::ErrorCode::RunCleanupUnconfirmed => ErrorCode::RunCleanupUnconfirmed,
        pb::ErrorCode::InternalError => ErrorCode::InternalError,
    };
    Ok(code)
}

fn malformed_failure() -> TalosError {
    TalosError::internal("Agent RPC failure payload is malformed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_tonic_deadline_is_not_a_business_cancellation() {
        let status = tonic::Status::from_error(Box::new(tonic::TimeoutExpired(())));
        assert_eq!(status.code(), tonic::Code::Cancelled);
        assert!(decode_status(status).is_code(ErrorCode::Timeout));
        assert!(decode_status(tonic::Status::cancelled("Caller cancelled"))
            .is_code(ErrorCode::Cancelled));
    }

    #[test]
    fn malformed_wire_errors_are_rejected_and_grpc_details_preserve_localization() {
        for localization in [
            ErrorLocalization::ModelNotConfigured {
                provider: "".into(),
                model: "m".into(),
            },
            ErrorLocalization::AttachmentCountExceeded {
                limit: 4,
                actual: 3,
            },
        ] {
            assert!(decode_api_error(encode_api_error(ApiError {
                code: localization.code(),
                message: "Reviewed public explanation".into(),
                localization: Some(localization),
            }))
            .is_err());
        }
        assert!(decode_api_error(pb::ApiError {
            code: pb::ErrorCode::InternalError as i32,
            message: "x".repeat(4097),
            localization: None,
        })
        .is_err());
        for localization in [
            ErrorLocalization::ModelNotConfigured {
                provider: "p".into(),
                model: "m".into(),
            },
            ErrorLocalization::ComputerDeletionActive,
            ErrorLocalization::ComputerRegistrationRemoved,
            ErrorLocalization::ComputerDeletionUnverified,
        ] {
            let original = TalosError::localized(localization);
            assert_eq!(
                decode_status(encode_status(&original)).to_api_error(),
                original.to_api_error()
            );
        }
    }

    #[test]
    fn localized_rpc_failure_uses_typed_parameters_instead_of_wire_english() {
        let failure = pb::Failure {
            error: Some(pb::ApiError {
                code: pb::ErrorCode::AttachmentLimitExceeded as i32,
                message: "Remote display text changed".into(),
                localization: Some(pb::api_error::Localization::AttachmentCountExceeded(
                    pb::AttachmentCountExceeded {
                        limit: 3,
                        actual: 4,
                    },
                )),
            }),
        };
        let decoded = decode_failure(failure).unwrap();
        assert!(decoded.is_code(ErrorCode::AttachmentLimitExceeded));
        assert!(matches!(
            decoded.to_api_error().localization,
            Some(ErrorLocalization::AttachmentCountExceeded {
                limit: 3,
                actual: 4
            })
        ));
    }
}
