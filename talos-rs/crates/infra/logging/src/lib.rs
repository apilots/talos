//! Talos Logging - 基于 tracing 的日志系统

use chrono_tz::Tz;
use std::path::{Path, PathBuf};
use talos_error::TalosResult;
use tracing::Dispatch;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod error_report;
pub use error_report::report_error;
mod config;
mod rotation_condition;
pub use config::{LogLevel, LogRotation, LoggingConfig};

/// Custom timer that formats timestamps in a specific IANA timezone.
struct TzTimer(Tz);

impl FormatTime for TzTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = chrono::Utc::now().with_timezone(&self.0);
        write!(w, "{}", now.format("%Y-%m-%dT%H:%M:%S%.3f%:z"))
    }
}

/// 日志守卫
#[derive(Debug, Default)]
pub struct LogGuard {
    _guards: Vec<WorkerGuard>,
}

/// Paths and process-entry behavior used to initialize logging before full config loading.
#[derive(Debug, Clone)]
pub struct StartupLoggingOptions {
    pub config_file: PathBuf,
    pub log_file: PathBuf,
    pub stdout: bool,
}

/// 初始化日志系统
pub fn init() -> TalosResult<LogGuard> {
    init_with_config(LoggingConfig::default(), "UTC")
}

/// Initialize logging once from the raw `logging` YAML node and stable startup defaults.
pub fn init_startup_once(options: StartupLoggingOptions) -> TalosResult<LogGuard> {
    let (config, timezone, warning) = startup_logging_config(&options);
    let guard = init_with_config(config, &timezone)?;
    if let Some(warning) = warning {
        tracing::warn!("{warning}");
    }
    tracing::info!(
        config_file = %options.config_file.display(),
        log_file = %options.log_file.display(),
        "startup logging initialized"
    );
    Ok(guard)
}

/// Initialize one logging owner with the application root timezone.
/// Timestamp formatting and calendar rotation share this timezone; file writing
/// and retention remain in the established appender/WorkerGuard lifecycle.
pub fn init_with_config(config: LoggingConfig, timezone: &str) -> TalosResult<LogGuard> {
    use talos_error::TalosError;
    let configured_file = config.file.clone();
    let level = config.level.clone();
    let (dispatch, guard) = build_dispatch(&config, timezone)?;
    tracing::dispatcher::set_global_default(dispatch).map_err(|e| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("initialize logging: {}", e),
        )
    })?;

    if let Some(path) = configured_file {
        tracing::info!("Log file configured: {}", path);
    }
    tracing::info!("Talos logging initialized (level={})", level);

    Ok(guard)
}

/// 使用默认配置初始化（保留旧接口名以兼容调用方）
pub fn init_from_env() -> TalosResult<LogGuard> {
    init_with_config(LoggingConfig::default(), "UTC")
}

fn startup_logging_config(
    options: &StartupLoggingOptions,
) -> (LoggingConfig, String, Option<String>) {
    let config = LoggingConfig {
        file: Some(options.log_file.to_string_lossy().to_string()),
        stdout: options.stdout,
        ..LoggingConfig::default()
    };

    let raw = match std::fs::read_to_string(&options.config_file) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (config, "UTC".into(), None)
        }
        Err(err) => {
            return (
                config,
                "UTC".into(),
                Some(format!(
                    "failed to read logging config {}: {err}",
                    options.config_file.display()
                )),
            )
        }
    };

    let root = match serde_yaml::from_str::<serde_yaml::Value>(&raw) {
        Ok(root) => root,
        Err(err) => {
            return (
                config,
                "UTC".into(),
                Some(format!(
                    "failed to parse logging config yaml {}: {err}",
                    options.config_file.display()
                )),
            )
        }
    };
    let timezone = root
        .get("timezone")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("UTC")
        .to_owned();
    let Some(logging) = root
        .as_mapping()
        .and_then(|mapping| mapping.get(serde_yaml::Value::String("logging".to_string())))
    else {
        return (config, timezone, None);
    };

    match serde_yaml::from_value::<LoggingConfig>(logging.clone()) {
        Ok(mut parsed) => {
            if parsed.file.is_none() {
                parsed.file = Some(options.log_file.to_string_lossy().to_string());
            }
            parsed.stdout = options.stdout;
            (parsed, timezone, None)
        }
        Err(err) => (
            config,
            timezone,
            Some(format!(
                "failed to decode logging config {}: {err}",
                options.config_file.display()
            )),
        ),
    }
}

fn build_dispatch(config: &LoggingConfig, timezone: &str) -> TalosResult<(Dispatch, LogGuard)> {
    use talos_error::TalosError;

    if !config.stdout && config.file.is_none() {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "logging requires at least one output: enable logging.stdout or set logging.file"
                .to_string(),
        ));
    }

    let timezone: Tz = timezone.trim().parse().map_err(|error| {
        TalosError::from_source(
            talos_error::ErrorCode::InvalidRequest,
            "The configured timezone is invalid.",
            error,
        )
    })?;
    let tz = (timezone != chrono_tz::UTC).then_some(timezone);

    let mut filter = EnvFilter::try_new(&config.level).map_err(|e| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid logging.level: {}", e),
        )
    })?;
    for (target, level) in &config.target_levels {
        if target.is_empty()
            || !target
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-'))
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "logging.target_levels requires tracing target names without span filters.",
            ));
        }
        let level: tracing::level_filters::LevelFilter = (*level).into();
        filter = filter.add_directive(format!("{target}={level}").parse().map_err(|error| {
            TalosError::from_source(
                talos_error::ErrorCode::InvalidRequest,
                "The logging target threshold is invalid.",
                error,
            )
        })?);
    }
    // Product handling-owner diagnostics retain their severity and stack contract.
    filter = filter.add_directive(
        "talos::diagnostic=warn"
            .parse()
            .expect("valid diagnostic directive"),
    );
    if config
        .rotation
        .as_ref()
        .is_some_and(|rotation| rotation.max_files < 2 || rotation.max_bytes == 0)
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "logging.rotation requires positive max_bytes and max_files >= 2.",
        ));
    }
    let (writer, guard) = build_writer(config, timezone)?;
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
            Dispatch::new(
                builder
                    .json()
                    .finish()
                    .with(tracing_subscriber::filter::filter_fn(
                        private_dependency_events,
                    )),
            )
        } else {
            Dispatch::new(builder.finish().with(tracing_subscriber::filter::filter_fn(
                private_dependency_events,
            )))
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
            Dispatch::new(
                builder
                    .json()
                    .finish()
                    .with(tracing_subscriber::filter::filter_fn(
                        private_dependency_events,
                    )),
            )
        } else {
            Dispatch::new(builder.finish().with(tracing_subscriber::filter::filter_fn(
                private_dependency_events,
            )))
        }
    };

    Ok((dispatch, guard))
}

// rmcp auth diagnostics include authorization codes and untrusted provider bodies.
// This static boundary cannot be overridden by user trace/span directives.
fn private_dependency_events(metadata: &tracing::Metadata<'_>) -> bool {
    let target = metadata.target();
    target != "rmcp::transport::auth" && !target.starts_with("rmcp::transport::auth::")
}

fn build_writer(config: &LoggingConfig, timezone: Tz) -> TalosResult<(BoxMakeWriter, LogGuard)> {
    match (config.stdout, config.file.as_deref()) {
        (true, Some(path)) => {
            let (file_writer, file_guard) =
                create_file_writer(path, config.rotation.as_ref(), timezone)?;
            Ok((
                BoxMakeWriter::new(std::io::stdout.and(file_writer)),
                LogGuard {
                    _guards: vec![file_guard],
                },
            ))
        }
        (true, None) => Ok((BoxMakeWriter::new(std::io::stdout), LogGuard::default())),
        (false, Some(path)) => {
            let (file_writer, file_guard) =
                create_file_writer(path, config.rotation.as_ref(), timezone)?;
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

fn create_file_writer(
    path: &str,
    rotation: Option<&LogRotation>,
    timezone: Tz,
) -> TalosResult<(NonBlocking, WorkerGuard)> {
    use talos_error::TalosError;

    let file_path = PathBuf::from(path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            TalosError::internal_with_source(
                "Failed to create the log directory; technical details were withheld.",
                err,
            )
            .context(format!("create log directory {}", parent.display()))
        })?;
    }

    let directory = file_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = file_path.file_name().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("invalid logging.file path: {}", path),
        )
    })?;
    if let Some(rotation) = rotation {
        let appender =
            rotation_condition::open(&file_path, rotation, timezone).map_err(|error| {
                TalosError::internal_with_source(
                    "The rotating log appender could not be opened.",
                    error,
                )
            })?;
        return Ok(file_writer_builder().finish(appender));
    }
    let appender = tracing_appender::rolling::never(directory, file_name);
    Ok(file_writer_builder().finish(appender))
}

/// Keep the file sink's lossless delivery policy independent of its writer.
fn file_writer_builder() -> NonBlockingBuilder {
    NonBlockingBuilder::default().lossy(false)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
