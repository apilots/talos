use std::fs;
use std::process::{Command, Stdio};

#[test]
fn startup_failures_are_reported_once_before_the_log_guard_drains() {
    for kind in ["Business", "Internal"] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let config = root.join("invalid.yaml");
        fs::write(root.join(".env"), "").unwrap();
        if kind == "Business" {
            fs::write(&config, "invalid: [").unwrap();
        } else {
            fs::create_dir(&config).unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_app-server"))
            .env_clear()
            .env("TALOS_HOME", root)
            .env("TALOS_CONFIG", config)
            .current_dir(root)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(
            output.stdout.is_empty(),
            "stdio protocol must not contain logs"
        );
        let log = fs::read_to_string(root.join("logs/talos.log")).unwrap();
        assert_eq!(log.matches("application failure").count(), 1);
        for expected in [kind, "sources=0:", "backtrace_status=Captured", "location="] {
            assert!(
                log.contains(expected),
                "missing {expected} in startup report: {log}"
            );
        }
    }
}
