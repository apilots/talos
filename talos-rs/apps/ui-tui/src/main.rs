use std::sync::Arc;

use talos_client::{AppServerClient, AppServerClientConfig};
use talos_types::talos::TalosConnectionMode;
use talos_ui_tui::run_tui_app_with_gateway_client;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let app_server_bin = option_arg(&args, "--app-server-bin")
        .or_else(|| std::env::var("TALOS_APP_SERVER_BIN").ok())
        .unwrap_or_else(|| "talos".to_string());
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
