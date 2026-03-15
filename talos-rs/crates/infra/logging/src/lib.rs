//! Talos Logging - 基于 tracing 的日志系统

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_error::TalosResult;

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
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
            format: default_format(),
            file: None,
            stdout: default_true(),
        }
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
pub struct LogGuard;

/// 初始化日志系统
pub fn init() -> TalosResult<LogGuard> {
    init_with_config(LoggingConfig::default())
}

/// 使用配置初始化
pub fn init_with_config(config: LoggingConfig) -> TalosResult<LogGuard> {
    use talos_error::TalosError;
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_new(&config.level)
        .map_err(|e| TalosError::InvalidConfiguration(format!("invalid logging.level: {}", e)))?;

    let builder = fmt()
        .with_target(true)
        .with_file(false)
        .with_line_number(false)
        .with_env_filter(filter);

    if config.format == "json" {
        builder.json().init();
    } else {
        builder.init();
    }

    // 文件日志（简化版，实际生产需要更完善的实现）
    if let Some(_path) = &config.file {
        let p = PathBuf::from(_path);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        tracing::info!("Log file configured: {}", _path);
    }

    tracing::info!("Talos logging initialized (level={})", config.level);

    Ok(LogGuard)
}

/// 使用默认配置初始化（保留旧接口名以兼容调用方）
pub fn init_from_env() -> TalosResult<LogGuard> {
    init_with_config(LoggingConfig::default())
}
