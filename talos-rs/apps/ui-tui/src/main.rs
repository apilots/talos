use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use talos_sdk::{
    now_ms, open_external_url, AppServerClient, AppServerClientConfig, CloudLoginClientKind,
    CloudLoginStatusResponse, TalosError,
};
use talos_ui_tui::run_tui_app_with_gateway_client;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let app_server_bin = resolve_app_server_bin(&args);
    let config = AppServerClientConfig {
        app_server_bin,
        client_name: "talos-ui-tui".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        actor_credentials: None,
        use_cloud_session_cache: false,
    }
    .with_cloud_session_cache();
    let mut client = AppServerClient::connect(config).await?;
    ensure_authenticated(&mut client).await?;
    let gateway_client: Arc<dyn talos_ui_tui::GatewayClient> = Arc::new(Mutex::new(client));
    run_tui_app_with_gateway_client(gateway_client).await
}

async fn ensure_authenticated(
    client: &mut AppServerClient,
) -> Result<(), Box<dyn std::error::Error>> {
    match client.current_user().await {
        Ok(_) => Ok(()),
        Err(TalosError::AuthError(_)) => login_with_cloud(client).await,
        Err(error) => Err(Box::new(error)),
    }
}

async fn login_with_cloud(client: &mut AppServerClient) -> Result<(), Box<dyn std::error::Error>> {
    let start = client
        .start_cloud_login("Talos TUI", CloudLoginClientKind::Tui)
        .await?;
    print_login_instructions(&start.login_url, &start.user_code);
    let browser_opened = std::env::var("TALOS_TUI_NO_BROWSER").is_err()
        && open_external_url(&start.login_url).is_ok();
    if browser_opened {
        eprintln!("Browser opened. If it did not appear, copy the URL above into your browser.");
    } else {
        eprintln!("Open the URL above in your browser to authorize Talos.");
    }

    loop {
        if now_ms() >= start.expires_at_ms {
            return Err("Cloud login expired".into());
        }
        match client.poll_cloud_login(start.login_id.clone()).await? {
            CloudLoginStatusResponse::Pending { interval_ms } => {
                eprint!("\rWaiting for Cloud authorization...");
                let _ = std::io::stderr().flush();
                tokio::time::sleep(Duration::from_millis(interval_ms.max(500))).await;
            }
            CloudLoginStatusResponse::Authenticated { user } => {
                eprintln!(
                    "\rAuthenticated as {}. Starting Talos TUI...",
                    user.username
                );
                clear_terminal();
                return Ok(());
            }
            CloudLoginStatusResponse::Denied => return Err("Cloud login was denied".into()),
            CloudLoginStatusResponse::Expired => return Err("Cloud login expired".into()),
        }
    }
}

fn print_login_instructions(login_url: &str, user_code: &str) {
    eprintln!();
    eprintln!("Talos Cloud login required.");
    eprintln!("Open this URL to authorize Talos:");
    eprintln!("{}", login_url);
    eprintln!("Code: {}", user_code);
    eprintln!();
}

fn clear_terminal() {
    eprint!("\x1b[2J\x1b[3J\x1b[H");
    let _ = std::io::stderr().flush();
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
