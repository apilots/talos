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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserCapabilityOperation {
    RemoteDesktop,
    TakeControl,
    ReleaseControl,
    OpenLocal,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BrowserCapabilityReason {
    NativeHost,
    NotBundled,
    HostPolicy,
    UnsupportedVersion,
    ClientMismatch,
}

/// Exact business explanations with real localized consumers.
/// Extend only when a domain has a stable, parameterized message; never add
/// arbitrary translation keys, diagnostic data, or presentation policy here.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
pub enum ErrorLocalization {
    BrowserCapabilityUnavailable {
        operation: BrowserCapabilityOperation,
        reason: BrowserCapabilityReason,
    },
    ComputerDeletionActive,
    ComputerRegistrationRemoved,
    ComputerDeletionUnverified,
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
            Self::BrowserCapabilityUnavailable { .. } => ErrorCode::InvalidRequest,
            Self::ComputerDeletionActive => ErrorCode::ActiveTurnRunning,
            Self::ComputerRegistrationRemoved => ErrorCode::ResourceExpired,
            Self::ComputerDeletionUnverified => ErrorCode::ServiceUnavailable,
            Self::ModelNotConfigured { .. } => ErrorCode::ProviderUnavailable,
            Self::AttachmentCountExceeded { .. } => ErrorCode::AttachmentLimitExceeded,
        }
    }

    pub fn english_message(&self) -> String {
        match self {
            Self::BrowserCapabilityUnavailable { operation, reason } => {
                let operation = match operation { BrowserCapabilityOperation::RemoteDesktop=>"Remote desktop", BrowserCapabilityOperation::TakeControl=>"Remote control", BrowserCapabilityOperation::ReleaseControl=>"Releasing remote control", BrowserCapabilityOperation::OpenLocal=>"Local browser presentation" };
                let reason = match reason { BrowserCapabilityReason::NativeHost=>"this Computer does not share its native desktop", BrowserCapabilityReason::NotBundled=>"the required runtime is not bundled on this Computer", BrowserCapabilityReason::HostPolicy=>"the host policy does not allow it", BrowserCapabilityReason::UnsupportedVersion=>"this runtime version does not support it", BrowserCapabilityReason::ClientMismatch=>"this client is not the browser's Desktop host" };
                format!("{operation} is unavailable: {reason}.")
            },
            Self::ComputerDeletionActive => "Finish or stop this Computer's active Turns before removing its registration.".into(),
            Self::ComputerRegistrationRemoved => "This Computer registration was removed. Use Connect a computer to enroll again.".into(),
            Self::ComputerDeletionUnverified => "Computer admission cannot be verified yet. Retry after its connection is restored or its lease expires.".into(),
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
            Self::BrowserCapabilityUnavailable { .. } => true,
            Self::ComputerDeletionActive
            | Self::ComputerRegistrationRemoved
            | Self::ComputerDeletionUnverified => true,
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
    /// Validate a received public failure without repairing malformed wire data.
    pub fn from_wire_parts(
        code: ErrorCode,
        message: String,
        localization: Option<ErrorLocalization>,
    ) -> crate::TalosResult<Self> {
        if message.trim().is_empty() || message.chars().count() > MAX_MESSAGE_CHARS {
            return Err(crate::TalosError::internal(
                "Public error message is invalid",
            ));
        }
        if localization
            .as_ref()
            .is_some_and(|value| !value.is_valid() || value.code() != code)
        {
            return Err(crate::TalosError::internal(
                "Public error localization is invalid",
            ));
        }
        Ok(Self {
            code,
            message,
            localization,
        })
    }

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
        Self::from_wire_parts(wire.code, wire.message, wire.localization)
            .map_err(|error| serde::de::Error::custom(error.to_api_error().message))
    }
}

#[cfg(test)]
#[path = "api_error_tests.rs"]
mod tests;
