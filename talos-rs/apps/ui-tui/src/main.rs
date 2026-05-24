use std::sync::Arc;

use talos_sdk::{AppServerClient, AppServerClientConfig};
use talos_types::talos::TalosConnectionMode;
use talos_ui_tui::run_tui_app_with_gateway_client;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let app_server_bin = resolve_app_server_bin(&args);
    let launch_token = option_arg(&args, "--launch-token")
        .or_else(|| std::env::var("TALOS_LOCAL_LAUNCH_TOKEN").ok())
        .unwrap_or_else(|| format!("tui-{}", uuid::Uuid::new_v4().simple()));
    let client = AppServerClient::connect(AppServerClientConfig {
        app_server_bin,
        launch_token,
        client_name: "talos-ui-tui".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
    })
    .await?;
    let gateway_client: Arc<dyn talos_ui_tui::GatewayClient> = Arc::new(Mutex::new(client));
    run_tui_app_with_gateway_client(gateway_client, TalosConnectionMode::Embedded).await
}

fn resolve_app_server_bin(args: &[String]) -> String {
    option_arg(args, "--app-server-bin")
        .or_else(|| std::env::var("TALOS_APP_SERVER_BIN").ok())
        .or_else(resolve_bundled_app_server_bin)
        .unwrap_or_else(|| {
            eprintln!(
                "missing App Server binary: expected bundled libexec/app-server, or set TALOS_APP_SERVER_BIN"
            );
            std::process::exit(1);
        })
}

fn resolve_bundled_app_server_bin() -> Option<String> {
    let current_exe = std::env::current_exe().ok()?;
    bundled_app_server_candidate(&current_exe).map(|path| path.to_string_lossy().into_owned())
}

fn bundled_app_server_candidate(current_exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let exe_dir = current_exe.parent()?;
    let package_dir = if exe_dir
        .file_name()
        .is_some_and(|name| name == std::ffi::OsStr::new("bin"))
    {
        exe_dir.parent()?
    } else {
        exe_dir
    };
    let candidate = package_dir
        .join("libexec")
        .join(app_server_executable_name());
    candidate.exists().then_some(candidate)
}

fn app_server_executable_name() -> &'static str {
    if cfg!(windows) {
        "app-server.exe"
    } else {
        "app-server"
    }
}

fn option_arg(args: &[String], key: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix(&format!("{}=", key)) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{app_server_executable_name, bundled_app_server_candidate, option_arg};
    use std::fs;

    #[test]
    fn option_arg_reads_space_and_equals_forms() {
        assert_eq!(
            option_arg(
                &args(&["talos", "--app-server-bin", "/tmp/app-server"]),
                "--app-server-bin"
            ),
            Some("/tmp/app-server".to_string())
        );
        assert_eq!(
            option_arg(
                &args(&["talos", "--app-server-bin=/tmp/app-server"]),
                "--app-server-bin"
            ),
            Some("/tmp/app-server".to_string())
        );
    }

    #[test]
    fn bundled_app_server_uses_package_libexec_from_bin_dir() {
        let temp = temp_dir("talos-tui-bin-layout");
        let bin_dir = temp.join("bin");
        let libexec_dir = temp.join("libexec");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&libexec_dir).unwrap();
        fs::write(libexec_dir.join(app_server_executable_name()), "").unwrap();

        let candidate = bundled_app_server_candidate(&bin_dir.join("talos")).unwrap();

        assert_eq!(candidate, libexec_dir.join(app_server_executable_name()));
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn bundled_app_server_returns_none_when_missing() {
        let temp = temp_dir("talos-tui-missing-app-server");
        let bin_dir = temp.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();

        assert!(bundled_app_server_candidate(&bin_dir.join("talos")).is_none());
        fs::remove_dir_all(temp).unwrap();
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::new_v4().simple()));
        assert!(!path.exists());
        path
    }
}
