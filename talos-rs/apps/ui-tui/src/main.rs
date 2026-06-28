use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use talos_config::Config;
use talos_sdk::{
    now_ms, open_external_url, CloudLoginClientKind, CloudLoginStartRequest,
    CloudLoginStatusRequest, CloudLoginStatusResponse, GatewayClient as SdkGatewayClient,
    TalosError, UserCurrentRequest,
};
use talos_sdk_embedded::{EmbeddedServerClient, EmbeddedServerClientConfig};
use talos_ui_tui::run_tui_app_with_gateway_client;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_embedded().await
}

async fn run_embedded() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;
    let logging_options = talos_base::TalosServerOptions {
        logging_stdout: Some(false),
        ..Default::default()
    };
    let _log_guard = talos_base::init_logging(&config, Some(&logging_options))?;
    let mut client = EmbeddedServerClient::connect(EmbeddedServerClientConfig::new(config)).await?;
    ensure_authenticated(&mut client).await?;
    let gateway_client: Arc<dyn talos_ui_tui::GatewayClient> = Arc::new(Mutex::new(client));
    run_tui_app_with_gateway_client(gateway_client).await
}

async fn ensure_authenticated<C>(client: &mut C) -> Result<(), Box<dyn std::error::Error>>
where
    C: SdkGatewayClient + Send,
{
    match SdkGatewayClient::current_user(client, UserCurrentRequest {}).await {
        Ok(_) => Ok(()),
        Err(TalosError::AuthError(_)) => login_with_cloud(client).await,
        Err(error) => Err(Box::new(error)),
    }
}

async fn login_with_cloud<C>(client: &mut C) -> Result<(), Box<dyn std::error::Error>>
where
    C: SdkGatewayClient + Send,
{
    let start = SdkGatewayClient::start_cloud_login(
        client,
        CloudLoginStartRequest {
            client_name: "Talos TUI".to_string(),
            client_kind: CloudLoginClientKind::Tui,
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
