use dotenv::dotenv;
use std::path::PathBuf;
use talos_config::Config;
use talos_server::{init_logging, run_json_rpc_stdio, start_server};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    dotenv().ok();

    let config_source = resolve_default_config_path();
    let config = Config::load().expect("failed to load config");
    config.validate_runtime().expect("invalid runtime config");

    if std::env::args().nth(1).as_deref() == Some("app-server") {
        if let Err(err) = run_json_rpc_stdio(config).await {
            eprintln!("app-server error: {err}");
            std::process::exit(1);
        }
        return;
    }

    let _log_guard = init_logging(&config, None).expect("failed to initialize logging");
    if let Some(path) = config_source {
        info!("Config loaded from {}", path.display());
    }
    info!(
        "Starting Talos server at {}:{}",
        config.gateway.host, config.gateway.port
    );

    let server = start_server(config, None)
        .await
        .expect("failed to start server runtime");

    if let Err(err) = server.run().await {
        error!("host error: {}", err);
    }
}

fn resolve_default_config_path() -> Option<PathBuf> {
    let config_path = Config::default_path();
    config_path
        .exists()
        .then(|| canonical_or_original(config_path))
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}
