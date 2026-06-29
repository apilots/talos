//! Talos Logging - 基于 tracing 的日志系统

use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use talos_error::TalosResult;
use tracing::Dispatch;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};
use tracing_subscriber::{fmt, EnvFilter};

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default = "default_true")]
    pub stdout: bool,
    /// IANA timezone name for log timestamps (e.g. "Asia/Shanghai"). Defaults to UTC.
    #[serde(default)]
    pub timezone: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
            format: default_format(),
            file: None,
            stdout: default_true(),
            timezone: None,
        }
    }
}

/// Custom timer that formats timestamps in a specific IANA timezone.
struct TzTimer(Tz);

impl FormatTime for TzTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = chrono::Utc::now().with_timezone(&self.0);
        write!(w, "{}", now.format("%Y-%m-%dT%H:%M:%S%.3f%:z"))
    }
}

fn default_level() -> String {
    "info".into()
}
fn default_format() -> String {
    "text".into()
}
fn default_true() -> bool {
    true
}

/// 日志守卫
#[derive(Debug, Default)]
pub struct LogGuard {
    _guards: Vec<WorkerGuard>,
}

/// 初始化日志系统
pub fn init() -> TalosResult<LogGuard> {
    init_with_config(LoggingConfig::default())
}

/// 使用配置初始化
pub fn init_with_config(config: LoggingConfig) -> TalosResult<LogGuard> {
    use talos_error::TalosError;
    let configured_file = config.file.clone();
    let level = config.level.clone();
    let (dispatch, guard) = build_dispatch(&config)?;
    tracing::dispatcher::set_global_default(dispatch)
        .map_err(|e| TalosError::InvalidConfiguration(format!("initialize logging: {}", e)))?;

    if let Some(path) = configured_file {
        tracing::info!("Log file configured: {}", path);
    }
    tracing::info!("Talos logging initialized (level={})", level);

    Ok(guard)
}

/// 使用默认配置初始化（保留旧接口名以兼容调用方）
pub fn init_from_env() -> TalosResult<LogGuard> {
    init_with_config(LoggingConfig::default())
}

fn build_dispatch(config: &LoggingConfig) -> TalosResult<(Dispatch, LogGuard)> {
    use talos_error::TalosError;

    if !config.stdout && config.file.is_none() {
        return Err(TalosError::InvalidConfiguration(
            "logging requires at least one output: enable logging.stdout or set logging.file"
                .to_string(),
        ));
    }

    let tz: Option<Tz> = config
        .timezone
        .as_deref()
        .filter(|s| !s.is_empty() && *s != "UTC")
        .and_then(|s| s.parse::<Tz>().ok());

    let filter = EnvFilter::try_new(&config.level)
        .map_err(|e| TalosError::InvalidConfiguration(format!("invalid logging.level: {}", e)))?;
    let (writer, guard) = build_writer(config)?;
    let enable_ansi = config.stdout && config.file.is_none() && config.format != "json";

    let dispatch = if let Some(tz) = tz {
        let builder = fmt()
            .with_target(true)
            .with_file(false)
            .with_line_number(false)
            .with_ansi(enable_ansi)
            .with_env_filter(filter)
            .with_writer(writer)
            .with_timer(TzTimer(tz));
        if config.format == "json" {
            Dispatch::new(builder.json().finish())
        } else {
            Dispatch::new(builder.finish())
        }
    } else {
        let builder = fmt()
            .with_target(true)
            .with_file(false)
            .with_line_number(false)
            .with_ansi(enable_ansi)
            .with_env_filter(filter)
            .with_writer(writer);
        if config.format == "json" {
            Dispatch::new(builder.json().finish())
        } else {
            Dispatch::new(builder.finish())
        }
    };

    Ok((dispatch, guard))
}

fn build_writer(config: &LoggingConfig) -> TalosResult<(BoxMakeWriter, LogGuard)> {
    match (config.stdout, config.file.as_deref()) {
        (true, Some(path)) => {
            let (file_writer, file_guard) = create_file_writer(path)?;
            Ok((
                BoxMakeWriter::new(std::io::stdout.and(file_writer)),
                LogGuard {
                    _guards: vec![file_guard],
                },
            ))
        }
        (true, None) => Ok((BoxMakeWriter::new(std::io::stdout), LogGuard::default())),
        (false, Some(path)) => {
            let (file_writer, file_guard) = create_file_writer(path)?;
            Ok((
                BoxMakeWriter::new(file_writer),
                LogGuard {
                    _guards: vec![file_guard],
                },
            ))
        }
        (false, None) => unreachable!("validated above"),
    }
}

fn create_file_writer(path: &str) -> TalosResult<(NonBlocking, WorkerGuard)> {
    use talos_error::TalosError;

    let file_path = PathBuf::from(path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            TalosError::StorageError(format!("create log dir {}: {}", parent.display(), err))
        })?;
    }

    let directory = file_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = file_path.file_name().ok_or_else(|| {
        TalosError::InvalidConfiguration(format!("invalid logging.file path: {}", path))
    })?;
    let appender = tracing_appender::rolling::never(directory, file_name);
    Ok(tracing_appender::non_blocking(appender))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
