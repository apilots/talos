//! Talos Command Line Interface Binary
//!
//! This binary provides the main command-line interface for the Talos system.
//! It uses the talos-sdk library to communicate with the gateway.

use tracing_subscriber;
mod cli;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Run the CLI
    match cli::run_cli().await {
        Ok(()) => {
            println!("Command executed successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error executing command: {}", e);
            std::process::exit(1);
        }
    }
}
