//! Shared bounds for canonical tool content and the final model projection.
use serde_json::{json, Value};
use talos_types::TalosContentItem;

/// A UTF-8 byte is a conservative text-token upper bound. Do not use chars/4
/// for a hard limit, especially for CJK or escaped JSON.
pub const MAX_MODEL_TOOL_BYTES: usize = 10_000;
const MARKER: &str = "...[truncated]";

pub fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let suffix = &MARKER[..MARKER.len().min(max_bytes)];
    let mut end = max_bytes.saturating_sub(suffix.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &text[..end])
}

fn truncate_tail(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let prefix = &MARKER[..MARKER.len().min(max_bytes)];
    let mut start = text.len() - max_bytes.saturating_sub(prefix.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{prefix}{}", &text[start..])
}

fn shrink(value: &mut Value, width: usize) {
    match value {
        Value::String(text) => *text = truncate_text(text, width),
        Value::Array(items) => {
            if items.len() > 1 {
                items.truncate(items.len().div_ceil(2));
            }
            for item in items {
                shrink(item, width);
            }
        }
        Value::Object(fields) => {
            let bash_output = fields.get("type").and_then(Value::as_str) == Some("bash_output");
            let file_read = fields.get("type").and_then(Value::as_str) == Some("file_read");
            let search_results =
                fields.get("type").and_then(Value::as_str) == Some("search_results");
            let matches_before = fields
                .get("matches")
                .and_then(Value::as_array)
                .filter(|_| search_results)
                .map(Vec::len);
            let mut text_trimmed = false;
            for (key, item) in fields.iter_mut() {
                if bash_output && matches!(key.as_str(), "stdout" | "stderr") {
                    if let Value::String(text) = item {
                        text_trimmed |= text.len() > width;
                        *text = truncate_tail(text, width);
                        continue;
                    }
                }
                if file_read && key == "content" {
                    text_trimmed |= item.as_str().is_some_and(|text| text.len() > width);
                }
                // Resource identities must remain usable after truncation.
                if !matches!(
                    key.as_str(),
                    "path"
                        | "relativePath"
                        | "fileId"
                        | "type"
                        | "status"
                        | "captureStatus"
                        | "code"
                        | "error_kind"
                ) {
                    shrink(item, width);
                }
            }
            if text_trimmed {
                fields.insert("truncated".into(), Value::Bool(true));
                if file_read {
                    // A producer's cursor can no longer describe bytes actually
                    // displayed after a last-resort crop. Never skip unseen data.
                    fields.insert("nextByteOffset".into(), Value::Null);
                }
            }
            if matches_before.is_some_and(|before| {
                fields
                    .get("matches")
                    .and_then(Value::as_array)
                    .is_some_and(|items| items.len() < before)
            }) {
                fields.insert("more_available".into(), Value::Bool(true));
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Keep valid JSON under the actual serialized budget, including escaping.
/// Oversized opaque objects are replaced explicitly rather than sliced JSON.
pub fn bounded_json(mut value: Value, max_bytes: usize) -> String {
    if max_bytes < 4 {
        return if max_bytes == 0 {
            String::new()
        } else {
            "0".to_owned()
        };
    }
    let mut width = (max_bytes / 2).max(MARKER.len());
    loop {
        let serialized = value.to_string();
        if serialized.len() <= max_bytes {
            return serialized;
        }
        if width == 0 {
            let omitted = json!({"truncated": true}).to_string();
            return if omitted.len() <= max_bytes {
                omitted
            } else {
                "null".to_owned()
            };
        }
        shrink(&mut value, width);
        width = if width == MARKER.len() {
            0
        } else {
            (width / 2).max(MARKER.len())
        };
    }
}

/// Canonical content stays typed. Structured JSON can change shape when omitted;
/// other content keeps its required fields or becomes an explicit text notice.
pub fn limit_content_items(items: &mut Vec<TalosContentItem>, max_bytes: usize) {
    if serde_json::to_vec(items).is_ok_and(|bytes| bytes.len() <= max_bytes) {
        return;
    }
    let value = serde_json::to_value(&*items).unwrap_or(Value::Null);
    let limited = bounded_json(value, max_bytes);
    if let Ok(mut result) = serde_json::from_str::<Vec<TalosContentItem>>(&limited) {
        for item in &mut result {
            match item {
                TalosContentItem::FileRead { truncated, .. }
                | TalosContentItem::BashOutput { truncated, .. } => *truncated = true,
                TalosContentItem::SearchResults { more_available, .. } => *more_available = true,
                _ => {}
            }
        }
        *items = result;
    } else {
        *items = vec![TalosContentItem::Text {
            text: MARKER.to_owned(),
        }];
        if serde_json::to_vec(&*items).is_ok_and(|bytes| bytes.len() > max_bytes) {
            items.clear();
        }
    }
}

#[cfg(test)]
#[path = "output_budget_tests.rs"]
mod tests;
