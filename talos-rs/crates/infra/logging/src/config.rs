//! Logging policy belongs to configuration; the writer only consumes its limits.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
    pub file: Option<String>,
    pub stdout: bool,
    /// IANA timestamp timezone. None uses UTC.
    pub timezone: Option<String>,
    /// Exact tracing target thresholds, overriding `level` for those targets.
    pub target_levels: BTreeMap<String, LogLevel>,
    /// Daily framework rotation. None keeps the configured file unbounded.
    pub rotation: Option<LogRotation>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            format: "text".into(),
            file: None,
            stdout: true,
            timezone: None,
            target_levels: BTreeMap::from([(
                "loro_internal::oplog::change_store::block_encode".into(),
                LogLevel::Warn,
            )]),
            rotation: Some(LogRotation::default()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for tracing::level_filters::LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Off => Self::OFF,
            LogLevel::Error => Self::ERROR,
            LogLevel::Warn => Self::WARN,
            LogLevel::Info => Self::INFO,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Trace => Self::TRACE,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogRotation {
    /// Maximum daily files managed by tracing-appender. Must be positive.
    pub max_files: usize,
}

impl Default for LogRotation {
    fn default() -> Self {
        Self { max_files: 5 }
    }
}
