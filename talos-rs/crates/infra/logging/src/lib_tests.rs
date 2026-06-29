use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_log_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("talos-logging-{}-{}", std::process::id(), unique))
        .join(name)
}

#[test]
fn test_build_dispatch_rejects_empty_outputs() {
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "text".to_string(),
        file: None,
        stdout: false,
        ..Default::default()
    };

    let err = build_dispatch(&config).expect_err("empty logging outputs should fail");
    let message = err.to_string();
    assert!(message.contains("at least one output"));
}

#[test]
fn test_build_dispatch_writes_text_logs_to_file() {
    let log_path = temp_log_path("talos.log");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "text".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    let (dispatch, guard) = build_dispatch(&config).expect("dispatch should build");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!("file sink is active");
    });
    drop(dispatch);
    drop(guard);

    let content = fs::read_to_string(&log_path).expect("log file should exist");
    assert!(content.contains("file sink is active"));
}

#[test]
fn test_build_dispatch_writes_json_logs_to_file() {
    let log_path = temp_log_path("talos.json.log");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "json".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    let (dispatch, guard) = build_dispatch(&config).expect("dispatch should build");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(target: "talos::test", "json sink is active");
    });
    drop(dispatch);
    drop(guard);

    let content = fs::read_to_string(&log_path).expect("log file should exist");
    assert!(content.starts_with('{'));
    assert!(content.contains("\"fields\":{\"message\":\"json sink is active\"}"));
}

#[test]
fn test_build_dispatch_creates_parent_directory_for_file_logs() {
    let log_path = temp_log_path("nested/logs/talos.log");
    let parent = log_path.parent().expect("parent dir should exist");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "text".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    assert!(!parent.exists(), "test precondition failed");
    let (_dispatch, guard) = build_dispatch(&config).expect("dispatch should build");
    drop(guard);

    assert!(parent.exists(), "log parent directory should be created");
}
