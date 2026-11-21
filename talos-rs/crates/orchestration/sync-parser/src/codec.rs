//! Shared typed serialization and error conversion.
use serde::{de::DeserializeOwned, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_sync_types::SyncError;

pub(super) fn transcode<T: Serialize, U: DeserializeOwned>(source: &T) -> TalosResult<U> {
    serde_json::from_value(serde_json::to_value(source).map_err(codec_error)?).map_err(codec_error)
}

pub fn codec_error(error: serde_json::Error) -> TalosError {
    TalosError::internal_with_source(
        "Synchronize Talos data; technical details were withheld.",
        error,
    )
}

pub fn sync_error(error: SyncError) -> TalosError {
    transcode(&error.code)
        .and_then(|code| talos_error::ApiError::from_wire_parts(code, error.message, None))
        .map_or_else(|error| error, Into::into)
}
