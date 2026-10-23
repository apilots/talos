//! Merge explicitly configured wire extensions without replacing runtime history.
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use talos_error::{ErrorCode, TalosError, TalosResult};

/// Serialize once, then merge extension keys into the request object. The runtime
/// still exclusively owns conversation, tools, streaming and authentication.
pub fn serialize_with_parameters(
    request: impl Serialize,
    parameters: &BTreeMap<String, Value>,
) -> TalosResult<Value> {
    let mut body = serde_json::to_value(request).map_err(|source| {
        TalosError::internal_with_source("Failed to serialize model request.", source)
    })?;
    let object = body
        .as_object_mut()
        .ok_or_else(|| TalosError::internal("Model request must be an object"))?;
    for (key, value) in parameters {
        if key.trim().is_empty() || talos_types::is_managed_request_parameter(key) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Request parameter is reserved by the runtime",
            ));
        }
        object.insert(key.clone(), value.clone());
    }
    Ok(body)
}

/// Boolean thinking controls are explicit endpoint configuration, never inferred
/// from provider IDs, model names or URLs.
pub fn has_boolean_thinking_control(parameters: &BTreeMap<String, Value>) -> bool {
    parameters
        .get("enable_thinking")
        .is_some_and(Value::is_boolean)
        || parameters
            .get("chat_template_kwargs")
            .and_then(|value| value.get("enable_thinking"))
            .is_some_and(Value::is_boolean)
}

/// The current session selection takes precedence over configured example values.
pub fn apply_thinking_selection(parameters: &mut BTreeMap<String, Value>, enabled: bool) {
    if let Some(value) = parameters
        .get_mut("enable_thinking")
        .filter(|value| value.is_boolean())
    {
        *value = Value::Bool(enabled);
    }
    if let Some(value) = parameters
        .get_mut("chat_template_kwargs")
        .and_then(|value| value.get_mut("enable_thinking"))
        .filter(|value| value.is_boolean())
    {
        *value = Value::Bool(enabled);
    }
    if let Some(value) = parameters
        .get_mut("thinking")
        .and_then(|value| value.get_mut("type"))
        .filter(|value| matches!(value.as_str(), Some("enabled" | "disabled")))
    {
        *value = Value::String(if enabled { "enabled" } else { "disabled" }.to_string());
    }
}

#[cfg(test)]
#[path = "request_parameters_tests.rs"]
mod tests;
