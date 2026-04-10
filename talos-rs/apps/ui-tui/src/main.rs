use std::sync::Arc;

use dotenv::dotenv;
use talos_client::{ClientConfig, TalosClient};
use talos_config::Config;
use talos_error::TalosResult;
use talos_server::{init_logging, start_server, TalosServerOptions};
use talos_types::talos::TalosConnectionMode;
use talos_ui_tui::run_tui_app_with_client;
use tracing::error;

const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_GATEWAY_TOKEN: &str = "talos-local-test-token";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    let use_external_gateway = has_flag(&args, "--gateway") || has_flag(&args, "-g");

    if use_external_gateway {
        let gateway_url = option_arg(&args, "--gateway-url")
            .or_else(|| std::env::var("TALOS_GATEWAY_URL").ok())
            .unwrap_or_else(|| DEFAULT_GATEWAY_URL.to_string());
        let auth_token = option_arg(&args, "--auth-token")
            .or_else(|| std::env::var("GATEWAY_TOKEN").ok())
            .or_else(|| Some(DEFAULT_GATEWAY_TOKEN.to_string()));

        eprintln!("Connecting to external gateway: {}", gateway_url);

        let config = ClientConfig {
            gateway_url,
            auth_token,
            timeout: 30,
        };
        let client = Arc::new(TalosClient::new(config)?);
        run_tui_app_with_client(client, TalosConnectionMode::Remote).await?;
    } else {
        run_embedded().await?;
    }

    Ok(())
}

async fn run_embedded() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load().expect(
        "failed to load Talos config from <talos_home>/conf/talos.yaml — run `talos init` or use --gateway to connect to an external gateway",
    );
    config.validate_runtime().expect("invalid runtime config");

    let options = TalosServerOptions {
        host: Some("127.0.0.1".to_string()),
        port: Some(find_free_port().await?),
        logging_stdout: Some(false),
        auth_disabled: Some(true),
        enable_channels: Some(false),
    };

    let _log_guard = init_logging(&config, Some(&options)).expect("failed to initialize logging");
    let server = start_server(config, Some(options))
        .await
        .expect("failed to start embedded server runtime");
    let gateway_url = server.gateway_url().to_string();
    let http_server = server.into_http_server()?;

    tokio::spawn(async move {
        if let Err(err) = http_server.run().await {
            error!("embedded gateway error: {}", err);
        }
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let client_config = ClientConfig {
        gateway_url,
        auth_token: None,
        timeout: 30,
    };
    let client = Arc::new(TalosClient::new(client_config)?);
    run_tui_app_with_client(client, TalosConnectionMode::Embedded).await?;

    Ok(())
}

async fn find_free_port() -> TalosResult<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| talos_error::TalosError::NetworkError(err.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|err| talos_error::TalosError::NetworkError(err.to_string()))?
        .port();
    Ok(port)
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
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
