use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

mod cli;

use talos_error::ErrorCode;
use talos_sdk::{
    now_ms, open_external_url, CloudLoginClientKind, CloudLoginStartRequest,
    CloudLoginStatusRequest, CloudLoginStatusResponse, GatewayClient as SdkGatewayClient,
    UserCurrentRequest,
};
use talos_sdk_embedded::EmbeddedServerClient;
use talos_tui::run_tui_app_with_gateway_client;
use tokio::sync::Mutex;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Runtime teardown can log while dropping background tasks. Flush the
    // logging worker only after that runtime has been dropped.
    let mut log_guard = None;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run(&mut log_guard))
}

async fn run(
    log_guard: &mut Option<talos_logging::LogGuard>,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--browser-mcp") {
        if let Err(error) = talos_base::run_browser_mcp().await {
            talos_base::exit_with_error("Browser MCP failed", error);
        }
        return Ok(());
    }
    match cli::parse_terminal_mode() {
        cli::TerminalMode::Tui => run_embedded_tui(log_guard).await,
        cli::TerminalMode::Command(command) => {
            let mut client = create_embedded_client(log_guard).await?;
            ensure_authenticated(&mut client, CloudLoginClientKind::Cli, "Talos CLI").await?;
            cli::run_cli_with_client(command, client).await
        }
    }
}

async fn create_embedded_client(
    log_guard: &mut Option<talos_logging::LogGuard>,
) -> Result<EmbeddedServerClient, Box<dyn std::error::Error>> {
    let logging_options = talos_base::TalosServerOptions {
        logging_stdout: Some(false),
        ..Default::default()
    };
    let app_server = talos_base::run_embedded_app_server(Some(&logging_options)).await?;
    *log_guard = Some(app_server.log_guard);
    let client = EmbeddedServerClient::from_runtime(app_server.runtime).await?;
    Ok(client)
}

async fn run_embedded_tui(
    log_guard: &mut Option<talos_logging::LogGuard>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = create_embedded_client(log_guard).await?;
    ensure_authenticated(&mut client, CloudLoginClientKind::Tui, "Talos TUI").await?;
    let gateway_client: Arc<dyn talos_tui::GatewayClient> = Arc::new(Mutex::new(client));
    run_tui_app_with_gateway_client(gateway_client).await
}

async fn ensure_authenticated<C>(
    client: &mut C,
    client_kind: CloudLoginClientKind,
    client_name: &'static str,
) -> Result<(), Box<dyn std::error::Error>>
where
    C: SdkGatewayClient + Send,
{
    match SdkGatewayClient::current_user(client, UserCurrentRequest {}).await {
        Ok(_) => Ok(()),
        Err(error) if error.is_code(ErrorCode::Unauthenticated) => {
            login_with_cloud(client, client_kind, client_name).await
        }
        Err(error) => Err(Box::new(error)),
    }
}

async fn login_with_cloud<C>(
    client: &mut C,
    client_kind: CloudLoginClientKind,
    client_name: &'static str,
) -> Result<(), Box<dyn std::error::Error>>
where
    C: SdkGatewayClient + Send,
{
    let start = SdkGatewayClient::start_cloud_login(
        client,
        CloudLoginStartRequest {
            client_name: client_name.to_string(),
            client_kind,
        },
    )
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
        match SdkGatewayClient::cloud_login_status(
            client,
            CloudLoginStatusRequest {
                login_id: start.login_id.clone(),
            },
        )
        .await?
        {
            CloudLoginStatusResponse::Pending { interval_ms } => {
                eprint!("\rWaiting for Cloud authorization...");
                let _ = std::io::stderr().flush();
                tokio::time::sleep(Duration::from_millis(interval_ms.max(500))).await;
            }
            CloudLoginStatusResponse::Authenticated { user, .. } => {
                eprintln!("\rAuthenticated as {}. Starting Talos...", user.username);
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
