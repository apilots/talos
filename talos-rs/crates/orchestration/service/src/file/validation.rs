use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use calamine::{Reader, Xls, Xlsx};
use talos_config::FileStorageLimits;
use talos_error::{TalosError, TalosResult};

const MIME_SNIFF_BYTES: usize = 64 * 1024;

/// Validates complete text uploads within FileService's configured byte limit.
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
        match self.suffix.as_str() {
            ".doc" | ".docx" => return,
            ".csv" | ".xls" | ".xlsx" | ".ppt" | ".pptx" | ".md" | ".markdown" => {
                self.content.extend_from_slice(bytes);
                return;
            }
            _ => {}
        }
        // Preserve bounded sniffing for recognized binary uploads. Text needs
        // the complete body; FileService enforces its upload limit before observe.
        let prefix = bytes
            .len()
            .min(MIME_SNIFF_BYTES.saturating_sub(self.content.len()));
        self.content.extend_from_slice(&bytes[..prefix]);
        if infer::get(&self.content).is_some_and(|kind| !kind.mime_type().starts_with("text/")) {
            return;
        }
        self.content.extend_from_slice(&bytes[prefix..]);
    }

    pub(super) fn detected_mime_type(&self) -> Option<String> {
        let mime_type = match self.suffix.as_str() {
            ".doc" => "application/msword",
            ".docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ".csv" if is_csv(&self.content) => "text/csv",
            ".md" | ".markdown" if is_utf8_text(&self.content) => "text/markdown",
            ".xls" if Xls::new(Cursor::new(&self.content)).is_ok() => "application/vnd.ms-excel",
            ".xlsx" if Xlsx::new(Cursor::new(&self.content)).is_ok() => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            // infer only examines early ZIP entries. Valid presentation packages
            // may place ppt/presentation.xml after that window; the existing ZIP
            // reader resolves the authoritative central directory instead.
            ".pptx"
                if zip::ZipArchive::new(Cursor::new(&self.content))
                    .is_ok_and(|mut package| package.by_name("ppt/presentation.xml").is_ok()) =>
            {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            ".csv" | ".xls" | ".xlsx" | ".pptx" | ".md" | ".markdown" => return None,
            _ => {
                let detected = infer::get(&self.content);
                if let Some(kind) = detected
                    .as_ref()
                    .filter(|kind| !kind.mime_type().starts_with("text/"))
                {
                    return Some(kind.mime_type().to_string());
                }
                if !is_utf8_text(&self.content) {
                    return None;
                }
                if let Some(kind) = detected {
                    return Some(kind.mime_type().to_string());
                }
                match self.suffix.as_str() {
                    ".html" | ".htm" => "text/html",
                    ".json"
                        if serde_json::from_slice::<serde::de::IgnoredAny>(&self.content)
                            .is_ok() =>
                    {
                        "application/json"
                    }
                    _ => "text/plain",
                }
            }
        };
        Some(mime_type.to_string())
    }
}

pub(super) fn filename(filename: String, limits: &FileStorageLimits) -> TalosResult<String> {
    let filename = filename.trim().to_string();
    if filename.is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file name must not be empty".to_string(),
        ));
    }
    if filename.len() > limits.max_filename_bytes {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file name exceeds the configured byte limit".to_string(),
        ));
    }
    if filename
        .chars()
        .any(|character| matches!(character, '/' | '\\' | '\0') || character.is_control())
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file name contains a path separator or control character".to_string(),
        ));
    }
    Ok(filename)
}

pub(super) fn tags(tags: Vec<String>, limits: &FileStorageLimits) -> TalosResult<Vec<String>> {
    if tags.len() > limits.max_tags {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file tags exceed the configured count limit".to_string(),
        ));
    }
    let mut normalized = BTreeSet::new();
    for tag in tags {
        let tag = tag.trim().to_string();
        if tag.is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file tags must not be empty".to_string(),
            ));
        }
        if tag.len() > limits.max_tag_bytes {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file tag exceeds the configured byte limit".to_string(),
            ));
        }
        if !normalized.insert(tag) {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
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
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "file metadata exceeds the configured entry limit".to_string(),
        ));
    }
    for (key, value) in metadata {
        if key.is_empty() || key.trim() != key {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file metadata keys must be non-empty without surrounding whitespace".to_string(),
            ));
        }
        if key.len() > limits.max_metadata_key_bytes {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file metadata key exceeds the configured byte limit".to_string(),
            ));
        }
        if value.len() > limits.max_metadata_value_bytes {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "file metadata value exceeds the configured byte limit".to_string(),
            ));
        }
    }
    let encoded = serde_json::to_vec(metadata).map_err(|error| {
        TalosError::internal_with_source(
            "serialize file metadata; technical details were withheld.",
            error,
        )
    })?;
    if encoded.len() > limits.max_metadata_bytes {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
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

// Validate the whole upload within FileService's existing upload-size limit.
// A suffix or a valid sniffed prefix cannot turn a binary tail into text.
pub(crate) fn is_utf8_text(content: &[u8]) -> bool {
    std::str::from_utf8(content).is_ok_and(|text| {
        !text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    })
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
