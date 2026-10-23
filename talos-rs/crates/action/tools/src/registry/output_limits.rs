use serde_json::Value;
use tracing::warn;

use crate::{ToolContext, ToolDispatchOutcome, ToolOutputPayload};

const DEFAULT_TOOL_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const TOOL_OUTPUT_TRUNCATED_SUFFIX: &str = "\n...[truncated]";

pub(super) fn resolve_max_output_bytes(context: &ToolContext) -> usize {
    context
        .kernel_config()
        .map(|config| config.tool_result_max_bytes)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_TOOL_OUTPUT_MAX_BYTES)
}

fn truncate_text(input: &str, max_bytes: usize) -> (String, bool, usize, usize) {
    let raw_size = input.len();
    if raw_size <= max_bytes {
        return (input.to_string(), false, raw_size, raw_size);
    }

    if max_bytes == 0 {
        return (String::new(), true, raw_size, 0);
    }

    if max_bytes <= TOOL_OUTPUT_TRUNCATED_SUFFIX.len() {
        let suffix = &TOOL_OUTPUT_TRUNCATED_SUFFIX[..max_bytes];
        return (suffix.to_string(), true, raw_size, suffix.len());
    }

    let mut cutoff = max_bytes - TOOL_OUTPUT_TRUNCATED_SUFFIX.len();
    while cutoff > 0 && !input.is_char_boundary(cutoff) {
        cutoff -= 1;
    }

    let mut output = String::with_capacity(max_bytes);
    output.push_str(&input[..cutoff]);
    output.push_str(TOOL_OUTPUT_TRUNCATED_SUFFIX);
    let returned_size = output.len();

    (output, true, raw_size, returned_size)
}

fn truncate_structured_error(
    value: &mut Option<Value>,
    max_output_bytes: usize,
) -> Option<(usize, usize)> {
    let payload = value.as_mut()?;
    let object = payload.as_object_mut()?;
    let error_value = object.get_mut("error")?;
    let error_text = error_value.as_str()?;

    let (limited, truncated, raw_size, returned_size) = truncate_text(error_text, max_output_bytes);
    if truncated {
        *error_value = Value::String(limited);
        Some((raw_size, returned_size))
    } else {
        None
    }
}

pub(super) fn enforce_output_limits(
    tool_name: &str,
    mut result: ToolDispatchOutcome,
    max_output_bytes: usize,
) -> ToolDispatchOutcome {
    let Some(payload) = (match &mut result {
        ToolDispatchOutcome::Executed { payload } => Some(payload),
        ToolDispatchOutcome::Pending { .. } => None,
    }) else {
        return result;
    };
    let (content_truncated, content_raw_size, content_returned_size) = match payload {
        ToolOutputPayload::Function { content, .. }
        | ToolOutputPayload::Resource { content, .. }
        | ToolOutputPayload::Mcp { content, .. } => {
            let (limited, truncated, raw_size, returned_size) =
                truncate_text(content, max_output_bytes);
            *content = limited;
            (truncated, raw_size, returned_size)
        }
    };
    let mut error_raw_size = 0;
    let mut error_returned_size = 0;
    let mut error_truncated = false;
    let mut mcp_projections_removed = false;

    if let ToolOutputPayload::Function {
        structured_content, ..
    } = payload
    {
        if let Some((raw_size, returned_size)) =
            truncate_structured_error(structured_content, max_output_bytes)
        {
            error_truncated = true;
            error_raw_size = raw_size;
            error_returned_size = returned_size;
        }
    }

    if let ToolOutputPayload::Mcp {
        is_error: true,
        structured_content,
        content_items,
        ..
    } = payload
    {
        let mut remaining = max_output_bytes.saturating_sub(content_returned_size);
        if let Some(value) = structured_content.as_ref() {
            let size = serialized_size(value);
            if size > remaining {
                *structured_content = None;
                mcp_projections_removed = true;
            } else {
                remaining -= size;
            }
        }
        if !content_items.is_empty() {
            let size = serialized_size(content_items);
            if size > remaining {
                content_items.clear();
                mcp_projections_removed = true;
            }
        }
    }

    match payload {
        ToolOutputPayload::Function {
            structured_content,
            content_items,
            ..
        }
        | ToolOutputPayload::Mcp {
            structured_content,
            content_items,
            ..
        } => {
            crate::output_budget::limit_content_items(content_items, max_output_bytes);
            if structured_content
                .as_ref()
                .is_some_and(|value| serialized_size(value) > max_output_bytes)
            {
                // The canonical content is authoritative. Do not retain an
                // unbounded duplicate projection for failures or program callers.
                let error = structured_content
                    .as_ref()
                    .and_then(crate::output::structured_error_from_value);
                *structured_content = error.and_then(|error| {
                    let mut projection = serde_json::json!({
                        "error": error.message,
                        "error_kind": error.error_kind,
                        "retryable": error.retryable,
                        "suggested_action": error.suggested_action,
                        "gate_source": error.gate_source,
                    });
                    let mut limited =
                        crate::output_budget::bounded_json(projection.clone(), max_output_bytes);
                    if !serde_json::from_str::<Value>(&limited)
                        .is_ok_and(|value| value.get("error").is_some())
                    {
                        // Optional recovery prose must not crowd out the actual
                        // error and its rejection/retry classification.
                        let object = projection.as_object_mut().expect("error projection object");
                        object.remove("suggested_action");
                        object.remove("gate_source");
                        limited = crate::output_budget::bounded_json(projection, max_output_bytes);
                    }
                    serde_json::from_str(&limited).ok()
                });
            }
        }
        ToolOutputPayload::Resource { .. } => {}
    }

    if content_truncated || error_truncated || mcp_projections_removed {
        warn!(
            tool_name,
            max_output_bytes,
            output_raw_size = content_raw_size,
            output_returned_size = content_returned_size,
            error_raw_size,
            error_returned_size,
            mcp_projections_removed,
            "tool result exceeded size limit and was truncated"
        );
    }

    result
}

fn serialized_size(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
#[path = "output_limits_tests.rs"]
mod tests;
