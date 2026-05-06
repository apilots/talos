use dotenv::dotenv;
use std::path::PathBuf;
use talos_config::Config;
use talos_server::{init_logging, run_json_rpc_stdio, start_server, TalosServerOptions};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    dotenv().ok();

    let command = std::env::args().nth(1);
    if should_print_version(command.as_deref()) {
        println!("talos {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if should_print_help(command.as_deref()) {
        print_help();
        return;
    }

    let config_source = resolve_default_config_path();
    let config = Config::load().expect("failed to load config");
    config.validate_runtime().expect("invalid runtime config");
    let app_server_mode = command.as_deref() == Some("app-server");

    if app_server_mode {
        let logging_options = TalosServerOptions {
            logging_stdout: Some(false),
            ..TalosServerOptions::default()
        };
        let _log_guard =
            init_logging(&config, Some(&logging_options)).expect("failed to initialize logging");
        if let Some(path) = config_source {
            info!("Config loaded from {}", path.display());
        }
        if let Err(err) = run_json_rpc_stdio(config).await {
            error!("app-server error: {}", err);
            eprintln!("app-server error: {err}");
            std::process::exit(1);
        }
        return;
    }

    let server_options = TalosServerOptions {
        webui_dir: resolve_packaged_webui_dir(),
        ..TalosServerOptions::default()
    };
    let _log_guard =
        init_logging(&config, Some(&server_options)).expect("failed to initialize logging");
    if let Some(path) = config_source {
        info!("Config loaded from {}", path.display());
    }
    info!(
        "Starting Talos server at {}:{}",
        config.gateway.host, config.gateway.port
    );

    let server = start_server(config, Some(server_options))
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

fn resolve_packaged_webui_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))?;
    let webui_dir = exe_dir.join("webui");
    webui_dir.join("index.html").exists().then_some(webui_dir)
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn should_print_version(arg: Option<&str>) -> bool {
    matches!(arg, Some("--version" | "-V" | "version"))
}

fn should_print_help(arg: Option<&str>) -> bool {
    matches!(arg, Some("--help" | "-h" | "help"))
}

fn print_help() {
    println!(
        "Talos {}\n\nUsage:\n  talos [app-server]\n  talos --version\n  talos --help",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::{should_print_help, should_print_version};

    #[test]
    fn version_args_do_not_require_runtime_config() {
        assert!(should_print_version(Some("--version")));
        assert!(should_print_version(Some("-V")));
        assert!(should_print_version(Some("version")));
        assert!(!should_print_version(Some("app-server")));
    }

    #[test]
    fn help_args_do_not_require_runtime_config() {
        assert!(should_print_help(Some("--help")));
        assert!(should_print_help(Some("-h")));
        assert!(should_print_help(Some("help")));
        assert!(!should_print_help(Some("app-server")));
    }
}
