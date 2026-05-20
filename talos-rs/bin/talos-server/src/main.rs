#[tokio::main]
async fn main() {
    let command = std::env::args().nth(1);
    if talos_base::should_print_version(command.as_deref()) {
        println!("talos-server {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if talos_base::should_print_help(command.as_deref()) {
        print_help();
        return;
    }
    if let Some(command) = command {
        eprintln!("unknown talos-server argument: {command}");
        print_help();
        std::process::exit(2);
    }

    if let Err(error) = talos_base::run_server().await {
        talos_base::exit_with_error("talos-server error", error);
    }
}

fn print_help() {
    println!(
        "Talos Server {}\n\nUsage:\n  talos-server\n  talos-server --version\n  talos-server --help",
        env!("CARGO_PKG_VERSION")
    );
}
