//! Example demonstrating the TUI interface capabilities
//!
//! Run with: cargo run -p talos-ui-tui --example tui_demo

fn main() {
    println!("Talos TUI Example");
    println!("=================");
    println!("The TUI now runs in gateway mode only.");
    println!();
    println!("To run the TUI:");
    println!("  # With embedded gateway (requires <talos_home>/conf/talos.yaml):");
    println!("  cargo run -p talos-ui-tui");
    println!();
    println!("  # With external gateway:");
    println!("  cargo run -p talos-ui-tui -- --gateway --gateway-url http://127.0.0.1:8080");
    println!();
    println!("Key features:");
    println!("- Welcome header with model/mode/workspace");
    println!("- Slash command support: /help /model /models /status /quit");
    println!("- Streaming conversation via Gateway SSE");
    println!("- Input prompt with keyboard shortcuts (Ctrl+C, Enter)");
}
