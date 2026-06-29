//! Example demonstrating the TUI interface capabilities
//!
//! Run with: cargo run -p talos-tui --example tui_demo

fn main() {
    println!("Talos TUI Example");
    println!("=================");
    println!("The TUI runs through the embedded Talos runtime.");
    println!();
    println!("To run the TUI:");
    println!("  # Embedded runtime (requires <talos_home>/conf/talos.yaml):");
    println!("  cargo run -p talos-tui --bin talos");
    println!();
    println!("Key features:");
    println!("- Welcome header with model/mode/workspace");
    println!("- Slash command support: /help /model /models /status /quit");
    println!("- Streaming conversation via shared timeline events");
    println!("- Input prompt with keyboard shortcuts (Ctrl+C, Enter)");
}
