use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use calamine::{Reader, Xls, Xlsx};
use talos_config::FileStorageLimits;
use talos_error::{TalosError, TalosResult};

const MIME_SNIFF_BYTES: usize = 64 * 1024;

/// Captures only the bytes required to validate one upload's media type.
///
/// Add a suffix-specific branch only when storage acceptance needs validation
/// that `infer` cannot provide.
pub(super) struct MediaTypeValidator {
    suffix: String,
    content: Vec<u8>,
}

impl MediaTypeValidator {
    pub(super) fn new(filename: &str) -> Self {
        Self {
            suffix: normalized_suffix(filename),
            content: Vec::new(),
        }
    }

    pub(super) fn observe(&mut self, bytes: &[u8]) {
        let capture_limit = match self.suffix.as_str() {
            ".csv" | ".xls" | ".xlsx" => usize::MAX,
            ".doc" | ".docx" => 0,
            _ => MIME_SNIFF_BYTES,
        };
        let remaining = capture_limit.saturating_sub(self.content.len());
        self.content
            .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
    }

    pub(super) fn detected_mime_type(&self) -> Option<String> {
        let mime_type = match self.suffix.as_str() {
            ".doc" => "application/msword",
            ".docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ".csv" if is_csv(&self.content) => "text/csv",
            ".xls" if Xls::new(Cursor::new(&self.content)).is_ok() => "application/vnd.ms-excel",
            ".xlsx" if Xlsx::new(Cursor::new(&self.content)).is_ok() => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            ".csv" | ".xls" | ".xlsx" => return None,
            _ => return infer::get(&self.content).map(|kind| kind.mime_type().to_string()),
        };
        Some(mime_type.to_string())
    }
}

pub(super) fn filename(filename: String, limits: &FileStorageLimits) -> TalosResult<String> {
    let filename = filename.trim().to_string();
    if filename.is_empty() {
        return Err(TalosError::ValidationError(
            "file name must not be empty".to_string(),
        ));
    }
    if filename.len() > limits.max_filename_bytes {
        return Err(TalosError::ValidationError(
            "file name exceeds the configured byte limit".to_string(),
        ));
    }
    if filename
        .chars()
        .any(|character| matches!(character, '/' | '\\' | '\0') || character.is_control())
    {
        return Err(TalosError::ValidationError(
            "file name contains a path separator or control character".to_string(),
        ));
    }
    Ok(filename)
}

pub(super) fn tags(tags: Vec<String>, limits: &FileStorageLimits) -> TalosResult<Vec<String>> {
    if tags.len() > limits.max_tags {
        return Err(TalosError::ValidationError(
            "file tags exceed the configured count limit".to_string(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for tag in tags {
        let tag = tag.trim().to_string();
        if tag.is_empty() {
            return Err(TalosError::ValidationError(
                "file tags must not be empty".to_string(),
            ));
        }
        if tag.len() > limits.max_tag_bytes {
            return Err(TalosError::ValidationError(
                "file tag exceeds the configured byte limit".to_string(),
            ));
        }
        if !normalized.insert(tag) {
            return Err(TalosError::ValidationError(
                "file tags must be unique after trimming".to_string(),
            ));
        }
    }
    Ok(normalized.into_iter().collect())
}

pub(super) fn metadata(
    metadata: &BTreeMap<String, String>,
    limits: &FileStorageLimits,
) -> TalosResult<()> {
    if metadata.len() > limits.max_metadata_entries {
        return Err(TalosError::ValidationError(
            "file metadata exceeds the configured entry limit".to_string(),
        ));
    }
    for (key, value) in metadata {
        if key.is_empty() || key.trim() != key {
            return Err(TalosError::ValidationError(
                "file metadata keys must be non-empty without surrounding whitespace".to_string(),
            ));
        }
        if key.len() > limits.max_metadata_key_bytes {
            return Err(TalosError::ValidationError(
                "file metadata key exceeds the configured byte limit".to_string(),
            ));
        }
        if value.len() > limits.max_metadata_value_bytes {
            return Err(TalosError::ValidationError(
                "file metadata value exceeds the configured byte limit".to_string(),
            ));
        }
    }
    let encoded = serde_json::to_vec(metadata).map_err(|error| {
        TalosError::SerializationError(format!("serialize file metadata: {error}"))
    })?;
    if encoded.len() > limits.max_metadata_bytes {
        return Err(TalosError::ValidationError(
            "file metadata exceeds the configured total byte limit".to_string(),
        ));
    }
    Ok(())
}

pub(super) fn normalized_suffix(filename: &str) -> String {
    let Some((_, suffix)) = filename.rsplit_once('.') else {
        return String::new();
    };
    let suffix = suffix.to_ascii_lowercase();
    if suffix.is_empty()
        || suffix.len() > 16
        || !suffix
            .bytes()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
    {
        return String::new();
    }
    format!(".{suffix}")
}

fn is_csv(content: &[u8]) -> bool {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(content);
    let mut row_count = 0;
    for row in reader.records() {
        if row.is_err() {
            return false;
        }
        row_count += 1;
    }
    row_count > 0
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
