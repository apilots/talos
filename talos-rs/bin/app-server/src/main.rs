#[tokio::main]
async fn main() {
    let command = std::env::args().nth(1);
    if talos_base::should_print_version(command.as_deref()) {
        println!("app-server {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if talos_base::should_print_help(command.as_deref()) {
        print_help();
        return;
    }
    if let Some(command) = command {
        eprintln!("unknown app-server argument: {command}");
        print_help();
        std::process::exit(2);
    }

    if let Err(error) = talos_base::run_app_server().await {
        talos_base::exit_with_error("app-server error", error);
    }
}

fn print_help() {
    println!(
        "App Server {}\n\nUsage:\n  app-server\n  app-server --version\n  app-server --help",
        env!("CARGO_PKG_VERSION")
    );
}
