use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ErrorCode;

const MAX_MESSAGE_CHARS: usize = 4096;
const MAX_PARAMETER_CHARS: usize = 256;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Public failure shared by requests, terminal events, SDKs, and history.
/// Message is a reviewed English explanation, never a diagnostic source dump.
#[derive(Debug, Clone, Serialize, TS, PartialEq, Eq)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    pub localization: Option<ErrorLocalization>,
}

impl schemars::JsonSchema for ApiError {
    fn schema_name() -> String {
        "ApiError".to_string()
    }

    fn json_schema(generator: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::{InstanceType, ObjectValidation, Schema, SchemaObject};
        Schema::Object(SchemaObject {
            instance_type: Some(InstanceType::Object.into()),
            object: Some(Box::new(ObjectValidation {
                properties: [
                    ("code".to_string(), generator.subschema_for::<ErrorCode>()),
                    ("message".to_string(), generator.subschema_for::<String>()),
                    (
                        "localization".to_string(),
                        generator.subschema_for::<Option<ErrorLocalization>>(),
                    ),
                ]
                .into_iter()
                .collect(),
                required: ["code", "message", "localization"]
                    .map(str::to_string)
                    .into_iter()
                    .collect(),
                additional_properties: Some(Box::new(Schema::Bool(false))),
                ..Default::default()
            })),
            ..Default::default()
        })
    }
}

/// Exact business explanations with real localized consumers.
/// Extend only when a domain has a stable, parameterized message; never add
/// arbitrary translation keys, diagnostic data, or presentation policy here.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
pub enum ErrorLocalization {
    ModelNotConfigured {
        provider: String,
        model: String,
    },
    AttachmentCountExceeded {
        #[ts(type = "number")]
        limit: u64,
        #[ts(type = "number")]
        actual: u64,
    },
}

impl ErrorLocalization {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ModelNotConfigured { .. } => ErrorCode::ProviderUnavailable,
            Self::AttachmentCountExceeded { .. } => ErrorCode::AttachmentLimitExceeded,
        }
    }

    pub fn english_message(&self) -> String {
        match self {
            Self::ModelNotConfigured { provider, model } => {
                format!("Model \"{model}\" is not configured for provider \"{provider}\".")
            }
            Self::AttachmentCountExceeded { limit, actual } => {
                format!("This request contains {actual} attachments; the limit is {limit}.")
            }
        }
    }

    fn is_valid(&self) -> bool {
        match self {
            Self::ModelNotConfigured { provider, model } => [provider, model].iter().all(|value| {
                !value.trim().is_empty() && value.chars().count() <= MAX_PARAMETER_CHARS
            }),
            Self::AttachmentCountExceeded { limit, actual } => {
                *limit <= MAX_SAFE_INTEGER && *actual <= MAX_SAFE_INTEGER && actual > limit
            }
        }
    }
}

impl ApiError {
    /// Use the code's standard explanation only when it fully describes the failure.
    pub fn new(code: ErrorCode) -> Self {
        Self::with_message(code, code.default_message())
    }

    /// Construct an unlocalized explanation reviewed by the owning domain adapter.
    pub fn with_message(code: ErrorCode, message: impl Into<String>) -> Self {
        let mut message = message.into();
        if message.trim().is_empty() {
            message = format!(
                "{}; no further explanation was available.",
                code.default_message()
            );
        }
        if message.chars().count() > MAX_MESSAGE_CHARS {
            const SUFFIX: &str = " [truncated]";
            message = message
                .chars()
                .take(MAX_MESSAGE_CHARS - SUFFIX.len())
                .collect();
            message.push_str(SUFFIX);
        }
        Self {
            code,
            message,
            localization: None,
        }
    }

    /// Derive code and English together; oversized parameters never change the cause.
    pub fn localized(localization: ErrorLocalization) -> Self {
        let mut error = Self::with_message(localization.code(), localization.english_message());
        if localization.is_valid() {
            error.localization = Some(localization);
        }
        error
    }
}

impl From<ErrorCode> for ApiError {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

impl<'de> Deserialize<'de> for ApiError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Decode-only shape: all validation belongs to this public boundary.
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            code: ErrorCode,
            message: String,
            #[serde(deserialize_with = "Option::deserialize")]
            localization: Option<ErrorLocalization>,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.message.trim().is_empty() || wire.message.chars().count() > MAX_MESSAGE_CHARS {
            return Err(serde::de::Error::custom("invalid public error message"));
        }
        if let Some(localization) = &wire.localization {
            if !localization.is_valid() || localization.code() != wire.code {
                return Err(serde::de::Error::custom("invalid error localization"));
            }
        }
        Ok(Self {
            code: wire.code,
            message: wire.message,
            localization: wire.localization,
        })
    }
}

#[cfg(test)]
#[path = "api_error_tests.rs"]
mod tests;
