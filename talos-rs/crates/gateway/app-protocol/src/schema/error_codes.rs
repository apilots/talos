//! Shared TypeScript runtime error-code exports.
use crate::errors::AppServerErrorCode;
use schemars::schema_for;

pub(super) fn export_app_server_error_code() -> String {
    let variants = [
        ("ParseError", AppServerErrorCode::ParseError),
        ("InvalidRequest", AppServerErrorCode::InvalidRequest),
        ("MethodNotFound", AppServerErrorCode::MethodNotFound),
        ("InvalidParams", AppServerErrorCode::InvalidParams),
        ("InternalError", AppServerErrorCode::InternalError),
        ("NotInitialized", AppServerErrorCode::NotInitialized),
        ("AlreadyInitialized", AppServerErrorCode::AlreadyInitialized),
    ];
    let union = variants
        .iter()
        .map(|(_, code)| code.as_i32().to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    let entries = variants
        .iter()
        .map(|(name, code)| format!("  {name}: {},", code.as_i32()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "export type AppServerErrorCode = {union};\n\
         export const AppServerErrorCode = {{\n{entries}\n\
         }} as const satisfies Record<string, AppServerErrorCode>;\n\n"
    )
}

pub(super) fn export_error_code_runtime_enum() -> String {
    let schema = schema_for!(talos_error::ErrorCode);
    let Some(values) = schema.schema.enum_values else {
        return String::new();
    };

    let entries = values
        .into_iter()
        .filter_map(|value| {
            value
                .as_str()
                .map(|code| (error_code_key(code), code.to_string()))
        })
        .map(|(key, code)| format!("  {key}: \"{code}\","))
        .collect::<Vec<_>>()
        .join("\n");

    format!("export const ErrorCode = {{\n{entries}\n}} as const satisfies Record<string, ErrorCode>;\n\n")
}

fn error_code_key(code: &str) -> String {
    let mut key = String::new();
    let mut uppercase_next = true;
    for ch in code.chars() {
        if ch == '_' || ch == '-' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            key.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            key.push(ch);
        }
    }
    key
}
