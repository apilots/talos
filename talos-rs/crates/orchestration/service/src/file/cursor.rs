use base64::Engine;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_store::file::FileListCursor;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileCursorPayload {
    created_at: i64,
    file_id: String,
}

pub(super) fn encode(cursor: &FileListCursor) -> TalosResult<String> {
    let payload = serde_json::to_vec(&FileCursorPayload {
        created_at: cursor.created_at,
        file_id: cursor.file_id.clone(),
    })
    .map_err(|error| TalosError::SerializationError(format!("encode file cursor: {error}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload))
}

pub(super) fn decode(cursor: &str) -> TalosResult<FileListCursor> {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| TalosError::ValidationError("invalid file cursor".to_string()))?;
    let payload: FileCursorPayload = serde_json::from_slice(&payload)
        .map_err(|_| TalosError::ValidationError("invalid file cursor".to_string()))?;
    if payload.file_id.is_empty() {
        return Err(TalosError::ValidationError(
            "invalid file cursor".to_string(),
        ));
    }
    Ok(FileListCursor {
        created_at: payload.created_at,
        file_id: payload.file_id,
    })
}
