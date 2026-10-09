#[tokio::main]
async fn main() {
    let command = std::env::args().nth(1);
    if matches!(command.as_deref(), Some("--version" | "-V" | "version")) {
        println!("talos-worker {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if matches!(command.as_deref(), Some("--help" | "-h" | "help")) {
        print_help();
        return;
    }
    if let Some(command) = command {
        eprintln!("unknown talos-worker argument: {command}");
        print_help();
        std::process::exit(2);
    }

    if let Err(error) = talos_worker::run_standalone().await {
        eprintln!("talos-worker error: {}", error.to_api_error().message);
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "Talos Worker {}\n\nUsage:\n  talos-worker\n  talos-worker --version\n  talos-worker --help",
        env!("CARGO_PKG_VERSION")
    );
}
