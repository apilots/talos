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

    if content_truncated || error_truncated {
        warn!(
            tool_name,
            max_output_bytes,
            output_raw_size = content_raw_size,
            output_returned_size = content_returned_size,
            error_raw_size,
            error_returned_size,
            "tool result exceeded size limit and was truncated"
        );
    }

    result
}
